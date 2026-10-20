// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The ACES 2.0 Output Transform, ported from OpenColorIO's reference implementation
//! (`src/OpenColorIO/ops/fixedfunction/ACES2`, BSD-3-Clause, Copyright Contributors to the OpenColorIO Project; see
//! `LICENSE-OpenColorIO` beside this file).
//!
//! The transform renders scene-referred linear Rec.709 light for a display whose gamut is Rec.709 (sRGB primaries, D65
//! white) and whose peak is `peak_luminance` nits, with 100 nits as SDR white:
//! 1. Convert to ACES AP1 with Bradford adaptation, clamp to the transform's input range, and convert to AP0
//!    (ACES2065-1), the space the model takes.
//! 2. Convert to JMh, the Hellwig 2022 appearance model's lightness, colorfulness, and hue in a dim surround.
//! 3. Tone map lightness through the Daniele Evo curve, scale colorfulness with it, and compress chroma toward the
//!    AP1 reach boundary.
//! 4. Compress colorfulness into the display gamut along lines toward a hue-dependent focus point, using the display
//!    gamut's cusp and an approximated upper hull per hue.
//! 5. Convert back to display RGB and clamp to the peak. One is 100 nits (SDR white), the engine's display convention.
//!
//! Steps 2–5 depend on tables of gamut cusps, upper-hull gammas, and reach boundaries per hue that are expensive to
//! find (iterative searches through the model), so [`Aces2`] builds them once per peak luminance on the CPU. The
//! display resolve uploads them with [`Aces2::uniforms`] and evaluates the per-pixel forward path in
//! `tone-mapping.wgsl`; [`Aces2::render`] evaluates the same path here for tests. Arithmetic is `f32`, as in the
//! reference, except primaries-derived matrices and inverses, which are built in `f64`.

use bytemuck::{
    Pod,
    Zeroable,
};

// ---------------------------------------------------------------------------------------------------------------------
// Color spaces
// ---------------------------------------------------------------------------------------------------------------------

/// A color space's red, green, and blue primaries and white point as CIE 1931 xy chromaticities.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Primaries {
    red: [f64; 2],
    green: [f64; 2],
    blue: [f64; 2],
    white: [f64; 2],
}

/// Rec.709 and sRGB: the engine's working and display primaries.
pub(crate) const REC709: Primaries = Primaries {
    red: [0.64, 0.33],
    green: [0.30, 0.60],
    blue: [0.15, 0.06],
    white: [0.3127, 0.3290],
};

/// Display P3 with a D65 white.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const P3_D65: Primaries = Primaries {
    red: [0.680, 0.320],
    green: [0.265, 0.690],
    blue: [0.150, 0.060],
    white: [0.3127, 0.3290],
};

/// ACES AP0 (ACES2065-1), the space the appearance model takes.
const ACES_AP0: Primaries = Primaries {
    red: [0.7347, 0.2653],
    green: [0.0, 1.0],
    blue: [0.0001, -0.0770],
    white: [0.32168, 0.33767],
};

/// ACES AP1 (ACEScg), the input range clamp and the chroma reach boundary.
const ACES_AP1: Primaries = Primaries {
    red: [0.713, 0.293],
    green: [0.165, 0.830],
    blue: [0.128, 0.044],
    white: [0.32168, 0.33767],
};

/// The appearance model's (non-physical) CAM16 cone primaries.
const CAM16: Primaries = Primaries {
    red: [0.8336, 0.1735],
    green: [2.3854, -1.4659],
    blue: [0.087, -0.125],
    white: [0.333, 0.333],
};

/// Bradford cone response, for chromatic adaptation between white points.
const BRADFORD: Matrix64 = [[0.8951, 0.2664, -0.1614], [-0.7502, 1.7135, 0.0367], [0.0389, -0.0685, 1.0296]];

// ---------------------------------------------------------------------------------------------------------------------
// Model constants (OpenColorIO `ACES2/Common.h`)
// ---------------------------------------------------------------------------------------------------------------------

/// Luminance, in nits, that RGB one represents: SDR white.
const REFERENCE_LUMINANCE: f32 = 100.0;
/// Adapting field luminance and relative background luminance of the viewing conditions.
const ADAPTING_LUMINANCE: f32 = 100.0;
const BACKGROUND_LUMINANCE: f32 = 20.0;
/// Dim surround: impact of surround (`c`) and chromatic induction (`N_c`).
const SURROUND_IMPACT: f32 = 0.59;
const SURROUND_INDUCTION: f32 = 0.9;
/// Lightness scale and the cone-response compression's offset and scale.
const J_SCALE: f32 = 100.0;
const COMPRESSION_OFFSET: f32 = 0.2713 * 100.0;
const COMPRESSION_SCALE: f32 = 4.0 * 100.0;

/// Chroma compression.
const CHROMA_COMPRESS: f32 = 2.4;
const CHROMA_COMPRESS_FACTOR: f32 = 3.3;
const CHROMA_EXPAND: f32 = 1.3;
const CHROMA_EXPAND_FACTOR: f32 = 0.69;
const CHROMA_EXPAND_THRESHOLD: f32 = 0.5;

/// Gamut compression.
const SMOOTH_CUSPS: f32 = 0.12;
const SMOOTH_M: f32 = 0.27;
const CUSP_MID_BLEND: f32 = 1.3;
const FOCUS_GAIN_BLEND: f32 = 0.3;
const FOCUS_DISTANCE: f32 = 1.35;
const FOCUS_DISTANCE_SCALING: f32 = 1.75;
#[cfg(test)]
const COMPRESSION_THRESHOLD: f32 = 0.75;

/// Upper-hull gamma search.
const GAMMA_MINIMUM: f32 = 0.0;
const GAMMA_MAXIMUM: f32 = 5.0;
const GAMMA_SEARCH_STEP: f32 = 0.4;
const GAMMA_ACCURACY: f32 = 1e-5;
const GAMMA_TEST_POSITIONS: [f32; 5] = [0.01, 0.1, 0.5, 0.8, 0.99];

/// Corners of the RGB cube that bound the gamut cusps (red, yellow, green, cyan, blue, magenta), and the cyclic table
/// that pads them with a wrapped copy at each end.
const CUSP_CORNERS: usize = 6;
const PADDED_CORNERS: usize = CUSP_CORNERS + 2;
const REACH_CUSP_TOLERANCE: f32 = 1e-3;
const DISPLAY_CUSP_TOLERANCE: f32 = 1e-7;

/// Hues span degrees; tables hold one nominal entry per degree plus one wrapped entry below and two above.
const HUE_LIMIT: f32 = 360.0;
const NOMINAL_ENTRIES: usize = 360;
const BASE_INDEX: usize = 1;
/// Entries per hue table, wrapping included.
pub(crate) const TABLE_ENTRIES: usize = NOMINAL_ENTRIES + 3;
const UPPER_WRAP_INDEX: usize = BASE_INDEX + NOMINAL_ENTRIES;
const LAST_NOMINAL_INDEX: usize = UPPER_WRAP_INDEX - 1;

// ---------------------------------------------------------------------------------------------------------------------
// Transform
// ---------------------------------------------------------------------------------------------------------------------

/// The ACES 2.0 Output Transform for one display peak: its model parameters and per-hue tables.
#[derive(Clone, Debug)]
pub(crate) struct Aces2 {
    peak_luminance: f32,
    /// Linear Rec.709 to AP1 (Bradford, D65 to the ACES white), and AP1 to AP0.
    rec709_to_ap1: Matrix,
    ap1_to_ap0: Matrix,
    /// AP1 values above this are clipped before rendering.
    input_limit: f32,
    input: JmhParameters,
    output: JmhParameters,
    tonescale: ToneScale,
    limit_j_max: f32,
    model_gamma_inverse: f32,
    reach_m: Vec<f32>,
    chroma: ChromaCompression,
    gamut: GamutCompression,
}

impl Aces2 {
    /// The transform for a Rec.709 display peaking at `peak_luminance` nits (100 for SDR).
    pub fn rec709(peak_luminance: f32) -> Self {
        Self::new(peak_luminance, &REC709)
    }

    /// The transform for a display whose gamut is `limiting` and which peaks at `peak_luminance` nits.
    pub fn new(peak_luminance: f32, limiting: &Primaries) -> Self {
        let rec709_to_ap1 = to_matrix(mul64(
            inverse64(rgb_to_xyz(&ACES_AP1)),
            mul64(bradford(REC709.white, ACES_AP1.white), rgb_to_xyz(&REC709)),
        ));
        let ap1_to_ap0 = to_matrix(rgb_to_rgb(&ACES_AP1, &ACES_AP0));

        let input = JmhParameters::new(&ACES_AP0);
        let output = JmhParameters::new(limiting);
        let reach = JmhParameters::new(&ACES_AP1);
        let tonescale = ToneScale::new(peak_luminance);
        let limit_j_max = y_to_j(peak_luminance, &input);
        let model_gamma_inverse = 1.0 / model_gamma();
        let reach_m = reach_m_table(&reach, limit_j_max);
        let chroma = ChromaCompression::new(peak_luminance, &tonescale);
        let gamut = GamutCompression::new(peak_luminance, &input, &output, &tonescale, limit_j_max, &reach);

        Self {
            peak_luminance,
            rec709_to_ap1,
            ap1_to_ap0,
            input_limit: tonescale.forward_limit,
            input,
            output,
            tonescale,
            limit_j_max,
            model_gamma_inverse,
            reach_m,
            chroma,
            gamut,
        }
    }

    #[cfg(test)]
    /// The display peak, in nits, these tables were built for.
    pub fn peak_luminance(&self) -> f32 {
        self.peak_luminance
    }

    #[cfg(test)]
    /// Render one linear Rec.709 scene color to display-linear RGB in the limiting primaries, where one is SDR white.
    pub fn render(&self, rec709: [f32; 3]) -> [f32; 3] {
        // Input: AP1 clamp, then AP0.
        let ap1 = mul(&self.rec709_to_ap1, rec709).map(|channel| channel.clamp(0.0, self.input_limit));
        let ap0 = mul(&self.ap1_to_ap0, ap1);

        // Appearance and tone scale.
        let aab = rgb_to_aab(ap0, &self.input);
        let jmh = aab_to_jmh(aab, &self.input);
        let reach_max_m = reach_m_from_table(jmh[2], &self.reach_m);
        let hue_radians = jmh[2].to_radians();
        let (sin_hue, cos_hue) = hue_radians.sin_cos();
        let m_norm = chroma_compress_norm(cos_hue, sin_hue, self.chroma.scale);
        let j_ts = tonescale_a_to_j(aab[0], &self.input, &self.tonescale);
        let compressed = self.chroma_compress(jmh, j_ts, m_norm, reach_max_m);
        let mapped = self.gamut_compress(compressed, reach_max_m);

        // Display.
        let a = j_to_achromatic(mapped[0], self.output.inverse_cz);
        let display_aab = [a, mapped[1] * cos_hue, mapped[1] * sin_hue];
        let rgb = aab_to_rgb(display_aab, &self.output);
        let peak = self.peak_luminance / REFERENCE_LUMINANCE;
        rgb.map(|channel| channel.clamp(0.0, peak))
    }

    /// Parameters and tables in the layout `AcesParameters` in `tone-mapping.wgsl` reads.
    pub fn uniforms(&self) -> AcesUniforms {
        let mut cusps = [[0.0; 4]; TABLE_ENTRIES];
        for (entry, (cusp, hue)) in cusps.iter_mut().zip(self.gamut.cusps.iter().zip(&self.gamut.hues)) {
            *entry = [cusp[0], cusp[1], cusp[2], *hue];
        }
        let mut reach = [[0.0; 4]; REACH_VECTORS];
        for (index, value) in self.reach_m.iter().enumerate() {
            reach[index / 4][index % 4] = *value;
        }
        let ap1_to_camera = mul_matrices(&self.input.rgb_to_cam, &self.ap1_to_ap0);
        AcesUniforms {
            rec709_to_ap1: columns(&self.rec709_to_ap1),
            ap1_to_camera: columns(&ap1_to_camera),
            input_cone_to_aab: columns(&self.input.cone_to_aab),
            output_aab_to_cone: columns(&self.output.aab_to_cone),
            output_camera_to_rgb: columns(&self.output.cam_to_rgb),
            model: [self.input.fl_n, self.input.cz, self.input.inverse_cz, self.input.a_w_j],
            limits: [
                self.input.inverse_a_w_j,
                self.input_limit,
                self.peak_luminance / REFERENCE_LUMINANCE,
                self.limit_j_max,
            ],
            tonescale: [self.tonescale.n_r, self.tonescale.g, self.tonescale.t_1, self.tonescale.s_2],
            compression: [self.tonescale.m_2, self.model_gamma_inverse, self.gamut.mid_j, self.gamut.focus_distance],
            chroma: [
                self.chroma.saturation,
                self.chroma.saturation_threshold,
                self.chroma.compression,
                self.chroma.scale,
            ],
            gamut: [
                self.gamut.lower_hull_gamma_inverse,
                self.gamut.search_range[0] as f32,
                self.gamut.search_range[1] as f32,
                0.0,
            ],
            cusps,
            reach,
        }
    }

    #[cfg(test)]
    /// Compress colorfulness after tone mapping, toward the reach boundary at the tone-mapped lightness.
    fn chroma_compress(&self, jmh: [f32; 3], j_ts: f32, m_norm: f32, reach_max_m: f32) -> [f32; 3] {
        let [j, m, h] = jmh;
        if m == 0.0 {
            return [j_ts, m, h];
        }
        let chroma = &self.chroma;
        let normalized_j = j_ts / self.limit_j_max;
        let shadow = (1.0 - normalized_j).max(0.0);
        let limit = normalized_j.powf(self.model_gamma_inverse) * reach_max_m / m_norm;

        let mut compressed = m * (j_ts / j).powf(self.model_gamma_inverse) / m_norm;
        compressed = limit
            - toe(
                limit - compressed,
                limit - 0.001,
                shadow * chroma.saturation,
                ((normalized_j * normalized_j) + chroma.saturation_threshold).sqrt(),
            );
        compressed = toe(compressed, limit, normalized_j * chroma.compression, shadow);
        [j_ts, compressed * m_norm, h]
    }

    #[cfg(test)]
    /// Compress colorfulness into the display gamut.
    fn gamut_compress(&self, jmh: [f32; 3], reach_max_m: f32) -> [f32; 3] {
        let [j, m, h] = jmh;
        if j <= 0.0 {
            return [0.0, 0.0, h];
        }
        if m <= 0.0 || j > self.limit_j_max {
            return [j, 0.0, h];
        }
        let gamut = &self.gamut;
        let hue = gamut.hue_dependent(h, self.limit_j_max);
        let slope_gain = focus_gain(j, hue.analytical_threshold, self.limit_j_max, gamut.focus_distance);
        let j_intersect_source = solve_j_intersect(j, m, hue.focus_j, self.limit_j_max, slope_gain);
        let slope = compression_vector_slope(j_intersect_source, hue.focus_j, self.limit_j_max, slope_gain);
        let j_intersect_cusp = solve_j_intersect(hue.cusp[0], hue.cusp[1], hue.focus_j, self.limit_j_max, slope_gain);
        let gamut_boundary_m = gamut_boundary_intersection(
            hue.cusp,
            self.limit_j_max,
            hue.gamma_top_inverse,
            gamut.lower_hull_gamma_inverse,
            j_intersect_source,
            slope,
            j_intersect_cusp,
        );
        if gamut_boundary_m <= 0.0 {
            return [j, 0.0, h];
        }
        let reach_boundary_m = line_boundary_intersection_m(
            j_intersect_source,
            slope,
            self.model_gamma_inverse,
            self.limit_j_max,
            reach_max_m,
            self.limit_j_max,
        );
        let remapped = remap_m(m, gamut_boundary_m, reach_boundary_m);
        [j_intersect_source + (remapped * slope), remapped, h]
    }
}

/// Number of `vec4f`s that hold the reach table, four entries each.
pub(crate) const REACH_VECTORS: usize = TABLE_ENTRIES.div_ceil(4);

/// `AcesParameters` in `tone-mapping.wgsl`: matrices as `mat3x3f` columns, then scalars, then the tables.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct AcesUniforms {
    rec709_to_ap1: [[f32; 4]; 3],
    ap1_to_camera: [[f32; 4]; 3],
    input_cone_to_aab: [[f32; 4]; 3],
    output_aab_to_cone: [[f32; 4]; 3],
    output_camera_to_rgb: [[f32; 4]; 3],
    /// F_L normalized, the lightness exponent and its reciprocal, and the achromatic white response.
    model: [f32; 4],
    /// The reciprocal white response, the AP1 input limit, the output peak (one is SDR white), and J at the peak.
    limits: [f32; 4],
    /// Tone scale `n_r`, `g`, `t_1`, and `s_2`.
    tonescale: [f32; 4],
    /// Tone scale `m_2`, the model gamma's reciprocal, middle-gray J, and the focus distance.
    compression: [f32; 4],
    /// Chroma expansion and its threshold, chroma compression, and the hue-dependent norm's scale.
    chroma: [f32; 4],
    /// The lower hull's gamma reciprocal and the hue search's lower and upper offsets.
    gamut: [f32; 4],
    /// Per hue-table entry: cusp J and M, upper-hull gamma reciprocal, and the entry's hue in degrees.
    cusps: [[f32; 4]; TABLE_ENTRIES],
    /// Reach boundary M per degree (wrapped like the hue table), four per vector.
    reach: [[f32; 4]; REACH_VECTORS],
}

// ---------------------------------------------------------------------------------------------------------------------
// Appearance model
// ---------------------------------------------------------------------------------------------------------------------

/// The appearance model's matrices and scalars for one set of primaries in the transform's viewing conditions.
#[derive(Clone, Debug)]
struct JmhParameters {
    /// RGB to adapted CAM16 cone responses (chromatic adaptation and the 100-nit scale folded in), and its inverse.
    rgb_to_cam: Matrix,
    cam_to_rgb: Matrix,
    /// Compressed cone responses to achromatic and opponent (Aab) responses, and its inverse.
    cone_to_aab: Matrix,
    aab_to_cone: Matrix,
    /// The luminance-level adaptation factor F_L over the reference luminance.
    fl_n: f32,
    /// The lightness nonlinearity exponent `c * z`, and its reciprocal.
    cz: f32,
    inverse_cz: f32,
    /// The achromatic response of the adapted white, and its reciprocal.
    a_w_j: f32,
    inverse_a_w_j: f32,
}

impl JmhParameters {
    fn new(primaries: &Primaries) -> Self {
        let base_cone_to_aab: Matrix =
            [2.0, 1.0, 1.0 / 20.0, 1.0, -12.0 / 11.0, 1.0 / 11.0, 1.0 / 9.0, 1.0 / 9.0, -2.0 / 9.0];

        let xyz_to_cam16 = to_matrix(inverse64(rgb_to_xyz(&CAM16)));
        let rgb_to_xyz = to_matrix(rgb_to_xyz(primaries));
        let white_xyz = mul(&rgb_to_xyz, [REFERENCE_LUMINANCE; 3]);
        let white_luminance = white_xyz[1];
        let white_cam16 = mul(&xyz_to_cam16, white_xyz);

        // Viewing-condition-dependent parameters.
        let k = 1.0 / ((5.0 * ADAPTING_LUMINANCE) + 1.0);
        let k4 = k * k * k * k;
        let fl = (0.2 * k4 * (5.0 * ADAPTING_LUMINANCE))
            + (0.1 * (1.0 - k4).powf(2.0) * (5.0 * ADAPTING_LUMINANCE).powf(1.0 / 3.0));
        let fl_n = fl / REFERENCE_LUMINANCE;
        let cz = model_gamma();

        // Discount the illuminant: scale cone responses so the white adapts to equal-energy.
        let adaptation = white_cam16.map(|channel| fl_n * white_luminance / channel);
        let adapted_white: [f32; 3] = std::array::from_fn(|channel| adaptation[channel] * white_cam16[channel]);
        let compressed_white = adapted_white.map(compress_cone_response);

        let cone_to_aab = mul_matrices(&diagonal([COMPRESSION_SCALE; 3]), &base_cone_to_aab);
        let white_achromatic = (cone_to_aab[0] * compressed_white[0])
            + (cone_to_aab[1] * compressed_white[1])
            + (cone_to_aab[2] * compressed_white[2]);
        let a_w_j = compress_cone_response_unsigned(fl);

        // Cone responses are prescaled to provide the chromatic adaptation directly.
        let rgb_to_cam16 = mul_matrices(&to_matrix(rgb_to_rgb(primaries, &CAM16)), &diagonal([REFERENCE_LUMINANCE; 3]));
        let rgb_to_cam = mul_matrices(&diagonal(adaptation), &rgb_to_cam16);
        let opponent = 43.0 * SURROUND_INDUCTION;
        let cone_to_aab = [
            cone_to_aab[0] / white_achromatic,
            cone_to_aab[1] / white_achromatic,
            cone_to_aab[2] / white_achromatic,
            cone_to_aab[3] * opponent,
            cone_to_aab[4] * opponent,
            cone_to_aab[5] * opponent,
            cone_to_aab[6] * opponent,
            cone_to_aab[7] * opponent,
            cone_to_aab[8] * opponent,
        ];

        Self {
            cam_to_rgb: invert(&rgb_to_cam),
            rgb_to_cam,
            aab_to_cone: invert(&cone_to_aab),
            cone_to_aab,
            fl_n,
            cz,
            inverse_cz: 1.0 / cz,
            a_w_j,
            inverse_a_w_j: 1.0 / a_w_j,
        }
    }
}

/// The lightness nonlinearity exponent `c * z` of the viewing conditions.
fn model_gamma() -> f32 {
    SURROUND_IMPACT * (1.48 + (BACKGROUND_LUMINANCE / REFERENCE_LUMINANCE).sqrt())
}

/// Post-adaptation cone response compression of a non-negative response.
fn compress_cone_response_unsigned(response: f32) -> f32 {
    let powered = response.powf(0.42);
    powered / (COMPRESSION_OFFSET + powered)
}

/// Inverse of [`compress_cone_response_unsigned`], limited below its asymptote.
fn expand_cone_response_unsigned(compressed: f32) -> f32 {
    let limited = compressed.min(0.99);
    ((COMPRESSION_OFFSET * limited) / (1.0 - limited)).powf(1.0 / 0.42)
}

/// Signed cone response compression: the magnitude compresses and the sign carries through.
fn compress_cone_response(response: f32) -> f32 {
    compress_cone_response_unsigned(response.abs()).copysign(response)
}

fn expand_cone_response(compressed: f32) -> f32 {
    expand_cone_response_unsigned(compressed.abs()).copysign(compressed)
}

fn achromatic_to_j(achromatic: f32, cz: f32) -> f32 {
    J_SCALE * achromatic.powf(cz)
}

fn j_to_achromatic(j: f32, inverse_cz: f32) -> f32 {
    (j * (1.0 / J_SCALE)).powf(inverse_cz)
}

#[cfg(test)]
/// Luminance (nits over the reference) of an achromatic response.
fn achromatic_to_y(achromatic: f32, parameters: &JmhParameters) -> f32 {
    expand_cone_response_unsigned(parameters.a_w_j * achromatic) / parameters.fl_n
}

fn y_to_j_unsigned(y: f32, parameters: &JmhParameters) -> f32 {
    let compressed = compress_cone_response_unsigned(y * parameters.fl_n);
    achromatic_to_j(compressed * parameters.inverse_a_w_j, parameters.cz)
}

fn y_to_j(y: f32, parameters: &JmhParameters) -> f32 {
    y_to_j_unsigned(y.abs(), parameters).copysign(y)
}

fn rgb_to_aab(rgb: [f32; 3], parameters: &JmhParameters) -> [f32; 3] {
    let cone = mul(&parameters.rgb_to_cam, rgb).map(compress_cone_response);
    mul(&parameters.cone_to_aab, cone)
}

fn aab_to_jmh(aab: [f32; 3], parameters: &JmhParameters) -> [f32; 3] {
    if aab[0] <= 0.0 {
        return [0.0; 3];
    }
    let j = achromatic_to_j(aab[0], parameters.cz);
    let m = ((aab[1] * aab[1]) + (aab[2] * aab[2])).sqrt();
    let mut hue = aab[2].atan2(aab[1]).to_degrees();
    if hue < 0.0 {
        hue += HUE_LIMIT;
    }
    [j, m, hue]
}

fn rgb_to_jmh(rgb: [f32; 3], parameters: &JmhParameters) -> [f32; 3] {
    aab_to_jmh(rgb_to_aab(rgb, parameters), parameters)
}

fn aab_to_rgb(aab: [f32; 3], parameters: &JmhParameters) -> [f32; 3] {
    let cone = mul(&parameters.aab_to_cone, aab).map(expand_cone_response);
    mul(&parameters.cam_to_rgb, cone)
}

fn jmh_to_rgb(jmh: [f32; 3], parameters: &JmhParameters) -> [f32; 3] {
    let (sin_hue, cos_hue) = jmh[2].to_radians().sin_cos();
    let aab = [j_to_achromatic(jmh[0], parameters.inverse_cz), jmh[1] * cos_hue, jmh[1] * sin_hue];
    aab_to_rgb(aab, parameters)
}

// ---------------------------------------------------------------------------------------------------------------------
// Tone scale and chroma compression
// ---------------------------------------------------------------------------------------------------------------------

/// The Daniele Evo tone scale's parameters for one display peak.
#[derive(Clone, Copy, Debug)]
struct ToneScale {
    /// Display peak and normalized white, in nits.
    n: f32,
    n_r: f32,
    /// Surround and contrast exponent, and the toe (flare) term.
    g: f32,
    t_1: f32,
    /// Display middle gray over the normalized white.
    c_t: f32,
    s_2: f32,
    m_2: f32,
    /// Scene values above this are clipped before rendering.
    forward_limit: f32,
    /// Decimal log of the peak over the normalized white.
    log_peak: f32,
}

impl ToneScale {
    fn new(peak_luminance: f32) -> Self {
        let n = peak_luminance;
        let n_r = 100.0; // normalized white in nits
        let g = 1.15; // surround and contrast
        let c = 0.18; // scene middle gray
        let c_d = 10.013; // display middle gray in nits
        let w_g = 0.14; // change in gray between peak luminances
        let t_1 = 0.04; // shadow toe or flare and glare compensation
        let r_hit_min = 128.0; // scene values "hitting the roof"
        let r_hit_max = 896.0;

        let r_hit = r_hit_min + ((r_hit_max - r_hit_min) * ((n / n_r).ln() / (10000.0_f32 / 100.0).ln()));
        let m_0 = n / n_r;
        let m_1 = 0.5 * (m_0 + (m_0 * (m_0 + (4.0 * t_1))).sqrt());
        let u = ((r_hit / m_1) / ((r_hit / m_1) + 1.0)).powf(g);
        let m = m_1 / u;
        let w_i = (n / 100.0).ln() / 2.0_f32.ln();
        let c_t = c_d / n_r * (1.0 + (w_i * w_g));
        let g_ip = 0.5 * (c_t + (c_t * (c_t + (4.0 * t_1))).sqrt());
        let g_ipp2 = -(m_1 * (g_ip / m).powf(1.0 / g)) / ((g_ip / m).powf(1.0 / g) - 1.0);
        let w_2 = c / g_ipp2;
        let s_2 = w_2 * m_1 * REFERENCE_LUMINANCE;
        let u_2 = ((r_hit / m_1) / ((r_hit / m_1) + w_2)).powf(g);
        let m_2 = m_1 / u_2;

        Self {
            n,
            n_r,
            g,
            t_1,
            c_t,
            s_2,
            m_2,
            forward_limit: 8.0 * r_hit,
            log_peak: (n / n_r).log10(),
        }
    }

    #[cfg(test)]
    /// Tone-mapped luminance of a scene luminance, both in nits over the reference.
    fn apply(&self, y: f32) -> f32 {
        let f = self.m_2 * (y / (y + self.s_2)).powf(self.g);
        (f * f / (f + self.t_1)).max(0.0) * self.n_r
    }
}

#[cfg(test)]
/// Tone-mapped lightness of an achromatic response.
fn tonescale_a_to_j(achromatic: f32, parameters: &JmhParameters, tonescale: &ToneScale) -> f32 {
    let y = achromatic_to_y(achromatic, parameters);
    y_to_j_unsigned(tonescale.apply(y), parameters).copysign(achromatic)
}

/// Chroma compression's derived parameters for one display peak.
#[derive(Clone, Copy, Debug)]
struct ChromaCompression {
    saturation: f32,
    saturation_threshold: f32,
    compression: f32,
    scale: f32,
}

impl ChromaCompression {
    fn new(peak_luminance: f32, tonescale: &ToneScale) -> Self {
        Self {
            compression: CHROMA_COMPRESS + (CHROMA_COMPRESS * CHROMA_COMPRESS_FACTOR * tonescale.log_peak),
            saturation: (CHROMA_EXPAND - (CHROMA_EXPAND * CHROMA_EXPAND_FACTOR * tonescale.log_peak)).max(0.2),
            saturation_threshold: CHROMA_EXPAND_THRESHOLD / tonescale.n,
            scale: (0.03379 * peak_luminance).powf(0.30596) - 0.45135,
        }
    }
}

#[cfg(test)]
/// Hue-dependent colorfulness normalization: a third-order Fourier series in hue.
fn chroma_compress_norm(cos_hue: f32, sin_hue: f32, scale: f32) -> f32 {
    let cos_2 = (2.0 * cos_hue * cos_hue) - 1.0;
    let sin_2 = 2.0 * cos_hue * sin_hue;
    let cos_3 = (4.0 * cos_hue * cos_hue * cos_hue) - (3.0 * cos_hue);
    let sin_3 = (3.0 * sin_hue) - (4.0 * sin_hue * sin_hue * sin_hue);
    let m = (11.34072 * cos_hue)
        + (16.46899 * cos_2)
        + (7.88380 * cos_3)
        + (14.66441 * sin_hue)
        + (-6.37224 * sin_2)
        + (9.19364 * sin_3)
        + 77.12896;
    m * scale
}

#[cfg(test)]
/// The reference's toe: values above `limit` pass through, values below roll off smoothly.
fn toe(x: f32, limit: f32, k1_in: f32, k2_in: f32) -> f32 {
    if x > limit {
        return x;
    }
    let k2 = k2_in.max(0.001);
    let k1 = ((k1_in * k1_in) + (k2 * k2)).sqrt();
    let k3 = (limit + k1) / (limit + k2);
    let minus_b = (k3 * x) - k1;
    let minus_ac = k2 * k3 * x;
    0.5 * (minus_b + ((minus_b * minus_b) + (4.0 * minus_ac)).sqrt())
}

// ---------------------------------------------------------------------------------------------------------------------
// Gamut compression
// ---------------------------------------------------------------------------------------------------------------------

/// Gamut compression's parameters and per-hue tables for one display peak.
#[derive(Clone, Debug)]
struct GamutCompression {
    mid_j: f32,
    focus_distance: f32,
    lower_hull_gamma_inverse: f32,
    /// Offsets bounding the hue search around a hue's nominal table position.
    search_range: [i32; 2],
    /// The hue sampled by each table entry (degrees, nearly uniform, wrapping entries outside [0, 360)).
    hues: Vec<f32>,
    /// Per entry: the display gamut cusp's J and M (M slightly expanded for the smooth minimum), and the upper hull's
    /// gamma reciprocal.
    cusps: Vec<[f32; 3]>,
}

#[cfg(test)]
/// One hue's interpolated gamut parameters.
struct HueGamut {
    cusp: [f32; 2],
    gamma_top_inverse: f32,
    focus_j: f32,
    analytical_threshold: f32,
}

impl GamutCompression {
    fn new(
        peak_luminance: f32,
        input: &JmhParameters,
        output: &JmhParameters,
        tonescale: &ToneScale,
        limit_j_max: f32,
        reach: &JmhParameters,
    ) -> Self {
        let mid_j = y_to_j(tonescale.c_t * REFERENCE_LUMINANCE, input);
        let focus_distance = FOCUS_DISTANCE + (FOCUS_DISTANCE * FOCUS_DISTANCE_SCALING * tonescale.log_peak);
        let lower_hull_gamma_inverse = 1.0 / (1.14 + (0.07 * tonescale.log_peak));

        // Sample hues as uniformly as possible while landing exactly on the display and reach gamut corners.
        let reach_corners = reach_corners(reach, limit_j_max, tonescale.forward_limit);
        let (display_rgb, display_jmh) = display_corners(output, peak_luminance);
        let sorted_hues = merge_corner_hues(&reach_corners, &display_jmh);
        let hues = hue_table(&sorted_hues);
        let mut cusps = cusp_table(&hues, &display_rgb, &display_jmh, output);
        let search_range = hue_search_range(&hues);
        upper_hull_gammas(
            &hues,
            &mut cusps,
            peak_luminance,
            limit_j_max,
            mid_j,
            focus_distance,
            lower_hull_gamma_inverse,
            output,
        );

        Self {
            mid_j,
            focus_distance,
            lower_hull_gamma_inverse,
            search_range,
            hues,
            cusps,
        }
    }

    #[cfg(test)]
    /// The cusp and hull parameters at `hue`, interpolated between table entries.
    fn hue_dependent(&self, hue: f32, limit_j_max: f32) -> HueGamut {
        let upper = lookup_hue_interval(hue, &self.hues, self.search_range);
        let t = (hue - self.hues[upper - 1]) / (self.hues[upper] - self.hues[upper - 1]);
        let lower_cusp = self.cusps[upper - 1];
        let upper_cusp = self.cusps[upper];
        let cusp: [f32; 3] = std::array::from_fn(|index| lerp(lower_cusp[index], upper_cusp[index], t));
        HueGamut {
            cusp: [cusp[0], cusp[1]],
            gamma_top_inverse: cusp[2],
            focus_j: focus_j(cusp[0], self.mid_j, limit_j_max),
            analytical_threshold: lerp(cusp[0], limit_j_max, FOCUS_GAIN_BLEND),
        }
    }
}

#[cfg(test)]
/// Index of the upper end of the table interval containing `hue`, searching near its nominal position.
fn lookup_hue_interval(hue: f32, hues: &[f32], search_range: [i32; 2]) -> usize {
    let mut index = (BASE_INDEX + hue as usize) as i32;
    let mut lower = (index + search_range[0]).max(0);
    let mut upper = (index + search_range[1]).min(UPPER_WRAP_INDEX as i32);
    while lower + 1 < upper {
        if hue > hues[index as usize] {
            lower = index;
        } else {
            upper = index;
        }
        index = (lower + upper) / 2;
    }
    upper.max(1) as usize
}

fn focus_j(cusp_j: f32, mid_j: f32, limit_j_max: f32) -> f32 {
    lerp(cusp_j, mid_j, (CUSP_MID_BLEND - (cusp_j / limit_j_max)).min(1.0))
}

fn focus_gain(j: f32, analytical_threshold: f32, limit_j_max: f32, focus_distance: f32) -> f32 {
    let gain = limit_j_max * focus_distance;
    if j <= analytical_threshold {
        return gain;
    }
    // Approximate inverse above the threshold, where J enters the calculation.
    let adjustment = ((limit_j_max - analytical_threshold) / (limit_j_max - j).max(0.0001)).log10();
    gain * ((adjustment * adjustment) + 1.0)
}

/// Where the compression line through (J, M) toward the focus crosses the J axis.
fn solve_j_intersect(j: f32, m: f32, focus_j: f32, max_j: f32, slope_gain: f32) -> f32 {
    let m_scaled = m / slope_gain;
    let a = m_scaled / focus_j;
    if j < focus_j {
        let b = 1.0 - m_scaled;
        let c = -j;
        let root = ((b * b) - (4.0 * a * c)).sqrt();
        return -2.0 * c / (b + root);
    }
    let b = -(1.0 + m_scaled + (max_j * a));
    let c = (max_j * m_scaled) + j;
    let root = ((b * b) - (4.0 * a * c)).sqrt();
    -2.0 * c / (b - root)
}

/// Smooth minimum about a scaled reference, from a cubic polynomial.
fn smooth_minimum(a: f32, b: f32, scale_reference: f32) -> f32 {
    let scaled = SMOOTH_CUSPS * scale_reference;
    let h = (scaled - (a - b).abs()).max(0.0) / scaled;
    a.min(b) - (h * h * h * scaled * (1.0 / 6.0))
}

fn compression_vector_slope(intersect_j: f32, focus_j: f32, limit_j_max: f32, slope_gain: f32) -> f32 {
    let direction = if intersect_j < focus_j { intersect_j } else { limit_j_max - intersect_j };
    direction * (intersect_j - focus_j) / (focus_j * slope_gain)
}

/// Approximate M where the line `J = slope * M + J_axis_intersect` meets the boundary `J = J_max * (M / M_max)^(1 /
/// inverse_gamma)`, without iterating.
fn line_boundary_intersection_m(
    j_axis_intersect: f32,
    slope: f32,
    inverse_gamma: f32,
    j_max: f32,
    m_max: f32,
    j_intersection_reference: f32,
) -> f32 {
    let normalized_j = j_axis_intersect / j_intersection_reference;
    let shifted = j_intersection_reference * normalized_j.powf(inverse_gamma);
    shifted * m_max / (j_max - (slope * m_max))
}

fn gamut_boundary_intersection(
    cusp: [f32; 2],
    j_max: f32,
    gamma_top_inverse: f32,
    gamma_bottom_inverse: f32,
    j_intersect_source: f32,
    slope: f32,
    j_intersect_cusp: f32,
) -> f32 {
    let lower = line_boundary_intersection_m(
        j_intersect_source,
        slope,
        gamma_bottom_inverse,
        cusp[0],
        cusp[1],
        j_intersect_cusp,
    );
    // The upper hull is flipped, zeroed at J max, with the slope negated.
    let upper = line_boundary_intersection_m(
        j_max - j_intersect_source,
        -slope,
        gamma_top_inverse,
        j_max - cusp[0],
        cusp[1],
        j_max - j_intersect_cusp,
    );
    smooth_minimum(lower, upper, cusp[1])
}

#[cfg(test)]
/// Reinhard-style compression of M between the gamut and reach boundaries above a threshold.
fn remap_m(m: f32, gamut_boundary_m: f32, reach_boundary_m: f32) -> f32 {
    let proportion = (gamut_boundary_m / reach_boundary_m).max(COMPRESSION_THRESHOLD);
    let threshold = proportion * gamut_boundary_m;
    if m <= threshold || proportion >= 1.0 {
        return m;
    }
    let m_offset = m - threshold;
    let gamut_offset = gamut_boundary_m - threshold;
    let reach_offset = reach_boundary_m - threshold;
    let scale = reach_offset / ((reach_offset / gamut_offset) - 1.0);
    let nd = m_offset / scale;
    threshold + (scale * nd / (1.0 + nd))
}

// ---------------------------------------------------------------------------------------------------------------------
// Table construction
// ---------------------------------------------------------------------------------------------------------------------

/// The reach boundary's M at the peak's J for every degree of hue, wrapped like the hue table.
fn reach_m_table(reach: &JmhParameters, limit_j_max: f32) -> Vec<f32> {
    const SEARCH_RANGE: f32 = 50.0;
    const SEARCH_MAXIMUM: f32 = 1300.0;
    let outside = |m: f32, hue: f32| jmh_to_rgb([limit_j_max, m, hue], reach).iter().any(|channel| *channel < 0.0);

    let mut table = vec![0.0; TABLE_ENTRIES];
    for position in 0..NOMINAL_ENTRIES {
        let hue = position as f32;
        let mut low = 0.0;
        let mut high = low + SEARCH_RANGE;
        while !outside(high, hue) && high < SEARCH_MAXIMUM {
            low = high;
            high += SEARCH_RANGE;
        }
        while high - low > 1e-2 {
            let middle = (high + low) / 2.0;
            if outside(middle, hue) {
                high = middle;
            } else {
                low = middle;
            }
        }
        table[position + BASE_INDEX] = high;
    }
    wrap_table(&mut table);
    table
}

#[cfg(test)]
/// Reach boundary M at `hue`, linear between whole degrees.
fn reach_m_from_table(hue: f32, table: &[f32]) -> f32 {
    let base = hue as usize;
    let t = hue - base as f32;
    let lower = base + BASE_INDEX;
    lerp(table[lower], table[lower + 1], t)
}

/// Copy the nominal ends into the wrapping entries.
fn wrap_table<T: Copy>(table: &mut [T]) {
    table[0] = table[LAST_NOMINAL_INDEX];
    table[UPPER_WRAP_INDEX] = table[BASE_INDEX];
    table[UPPER_WRAP_INDEX + 1] = table[BASE_INDEX + 1];
}

/// A unit RGB cube corner, in hue order red, yellow, green, cyan, blue, magenta.
fn unit_cube_corner(corner: usize) -> [f32; 3] {
    let lit = |offset: usize| if (corner + offset) % CUSP_CORNERS < 3 { 1.0 } else { 0.0 };
    [lit(1), lit(5), lit(3)]
}

/// The order that cycles six corners so the lowest hue sits at index one, with a wrapped copy padding each end.
fn corner_cycle(corners: &[[f32; 3]; CUSP_CORNERS]) -> [usize; PADDED_CORNERS] {
    let lowest = (0..CUSP_CORNERS).min_by(|a, b| corners[*a][2].total_cmp(&corners[*b][2])).unwrap_or(0);
    let mut order = [0; PADDED_CORNERS];
    for index in 0..CUSP_CORNERS {
        order[index + 1] = (index + lowest) % CUSP_CORNERS;
    }
    order[0] = order[CUSP_CORNERS];
    order[CUSP_CORNERS + 1] = order[1];
    order
}

/// JMh of the reach gamut's corners where they reach the peak's J.
fn reach_corners(reach: &JmhParameters, limit_j_max: f32, maximum_source: f32) -> [[f32; 3]; PADDED_CORNERS] {
    let limit_a = j_to_achromatic(limit_j_max, reach.inverse_cz);
    let mut corners = [[0.0; 3]; CUSP_CORNERS];
    for (index, corner) in corners.iter_mut().enumerate() {
        let direction = unit_cube_corner(index);
        let mut lower = 0.0;
        let mut upper = maximum_source;
        while upper - lower > REACH_CUSP_TOLERANCE {
            let test = (lower + upper) / 2.0;
            let achromatic = rgb_to_aab(direction.map(|channel| channel * test), reach)[0];
            if achromatic < limit_a {
                lower = test;
            } else {
                upper = test;
            }
            if achromatic == limit_a {
                break;
            }
        }
        *corner = rgb_to_jmh(direction.map(|channel| channel * upper), reach);
    }
    pad_corner_hues(corner_cycle(&corners).map(|index| corners[index]))
}

/// RGB and JMh of the display gamut's cusp corners at the peak.
fn display_corners(
    output: &JmhParameters,
    peak_luminance: f32,
) -> ([[f32; 3]; PADDED_CORNERS], [[f32; 3]; PADDED_CORNERS]) {
    let scale = peak_luminance / REFERENCE_LUMINANCE;
    let rgb: [[f32; 3]; CUSP_CORNERS] =
        std::array::from_fn(|index| unit_cube_corner(index).map(|channel| channel * scale));
    let jmh: [[f32; 3]; CUSP_CORNERS] = std::array::from_fn(|index| rgb_to_jmh(rgb[index], output));
    let order = corner_cycle(&jmh);
    (order.map(|index| rgb[index]), pad_corner_hues(order.map(|index| jmh[index])))
}

/// Shift the wrapped end copies' hues outside [0, 360) so the cycle stays monotonic.
fn pad_corner_hues(mut corners: [[f32; 3]; PADDED_CORNERS]) -> [[f32; 3]; PADDED_CORNERS] {
    corners[0][2] -= HUE_LIMIT;
    corners[CUSP_CORNERS + 1][2] += HUE_LIMIT;
    corners
}

/// The unique corner hues of both gamuts, merged in ascending order.
fn merge_corner_hues(reach: &[[f32; 3]; PADDED_CORNERS], display: &[[f32; 3]; PADDED_CORNERS]) -> Vec<f32> {
    let mut hues = Vec::with_capacity(2 * CUSP_CORNERS);
    let (mut reach_index, mut display_index) = (1, 1);
    while reach_index < CUSP_CORNERS + 1 || display_index < CUSP_CORNERS + 1 {
        let reach_hue = reach[reach_index][2];
        let display_hue = display[display_index][2];
        if reach_hue == display_hue {
            hues.push(reach_hue);
            reach_index += 1;
            display_index += 1;
        } else if reach_hue < display_hue {
            hues.push(reach_hue);
            reach_index += 1;
        } else {
            hues.push(display_hue);
            display_index += 1;
        }
    }
    hues
}

/// A hue table that samples evenly between consecutive corner hues, landing on each corner exactly.
fn hue_table(sorted_hues: &[f32]) -> Vec<f32> {
    let ideal_spacing = NOMINAL_ENTRIES as f32 / HUE_LIMIT;
    let mut sample_counts = vec![0_usize; sorted_hues.len()];
    let mut last_index = usize::MAX;
    let mut minimum_index = if sorted_hues[0] == 0.0 { 0 } else { 1 };
    for hue_index in 0..sorted_hues.len() {
        let mut nominal =
            ((sorted_hues[hue_index] * ideal_spacing).round() as usize).max(minimum_index).min(NOMINAL_ENTRIES - 1);
        if last_index == nominal {
            // Two hues landed on one entry: move the previous one down if possible, else this one up.
            if hue_index > 1 && sample_counts[hue_index - 2] != sample_counts[hue_index - 1].wrapping_sub(1) {
                sample_counts[hue_index - 1] -= 1;
            } else {
                nominal += 1;
            }
        }
        sample_counts[hue_index] = nominal.min(NOMINAL_ENTRIES - 1);
        last_index = nominal;
        minimum_index = nominal;
    }

    let mut table = vec![0.0; TABLE_ENTRIES];
    let mut fill = |samples: usize, lower: f32, upper: f32, start: usize| {
        let delta = (upper - lower) / samples as f32;
        for sample in 0..samples {
            table[start + sample] = lower + (sample as f32 * delta);
        }
    };
    let mut total = 0;
    fill(sample_counts[0], 0.0, sorted_hues[0], total + 1);
    total += sample_counts[0];
    let mut index = 1;
    while index < sorted_hues.len() {
        let samples = sample_counts[index] - sample_counts[index - 1];
        fill(samples, sorted_hues[index - 1], sorted_hues[index], total + 1);
        total += samples;
        index += 1;
    }
    fill(NOMINAL_ENTRIES - total, sorted_hues[index - 1], HUE_LIMIT, total + 1);

    table[0] = table[LAST_NOMINAL_INDEX] - HUE_LIMIT;
    table[UPPER_WRAP_INDEX] = table[BASE_INDEX] + HUE_LIMIT;
    table[UPPER_WRAP_INDEX + 1] = table[BASE_INDEX + 1] + HUE_LIMIT;
    table
}

/// The display gamut cusp's J and M at every table hue, found by bisecting along the cube edge between corners.
fn cusp_table(
    hues: &[f32],
    corner_rgb: &[[f32; 3]; PADDED_CORNERS],
    corner_jmh: &[[f32; 3]; PADDED_CORNERS],
    output: &JmhParameters,
) -> Vec<[f32; 3]> {
    let mut previous = (0_usize, 0.0_f32);
    let mut table = vec![[0.0; 3]; TABLE_ENTRIES];
    for index in BASE_INDEX..UPPER_WRAP_INDEX {
        let hue = hues[index];
        let [j, m] = display_cusp_for_hue(hue, corner_rgb, corner_jmh, output, &mut previous);
        table[index] = [j, m * (1.0 + (SMOOTH_M * SMOOTH_CUSPS)), hue];
    }
    wrap_table(&mut table);
    table
}

fn display_cusp_for_hue(
    hue: f32,
    corner_rgb: &[[f32; 3]; PADDED_CORNERS],
    corner_jmh: &[[f32; 3]; PADDED_CORNERS],
    output: &JmhParameters,
    previous: &mut (usize, f32),
) -> [f32; 2] {
    let upper_corner = (1..PADDED_CORNERS).find(|index| corner_jmh[*index][2] > hue).unwrap_or(1);
    let lower_corner = upper_corner - 1;
    if corner_jmh[lower_corner][2] == hue {
        return [corner_jmh[lower_corner][0], corner_jmh[lower_corner][1]];
    }

    // Bisect along the cube edge; continue from the previous hue's position while on the same edge.
    let along = |t: f32| -> [f32; 3] {
        std::array::from_fn(|channel| lerp(corner_rgb[lower_corner][channel], corner_rgb[upper_corner][channel], t))
    };
    let mut lower_t = if upper_corner == previous.0 { previous.1 } else { 0.0 };
    let mut upper_t = 1.0;
    while upper_t - lower_t > DISPLAY_CUSP_TOLERANCE {
        let sample_t = (lower_t + upper_t) / 2.0;
        let sample_hue = rgb_to_jmh(along(sample_t), output)[2];
        if sample_hue < corner_jmh[lower_corner][2] {
            upper_t = sample_t;
        } else if sample_hue >= corner_jmh[upper_corner][2] || sample_hue <= hue {
            lower_t = sample_t;
        } else {
            upper_t = sample_t;
        }
    }
    let sample_t = (lower_t + upper_t) / 2.0;
    let jmh = rgb_to_jmh(along(sample_t), output);
    *previous = (upper_corner, sample_t);
    [jmh[0], jmh[1]]
}

/// The largest deviations of the table hues from uniform spacing, padded, which bound the per-pixel hue search.
fn hue_search_range(hues: &[f32]) -> [i32; 2] {
    let mut range = [0, 1];
    for (index, hue) in hues.iter().enumerate().take(UPPER_WRAP_INDEX).skip(BASE_INDEX) {
        let nominal = (BASE_INDEX + *hue as usize) as i32;
        let delta = index as i32 - nominal;
        range[0] = range[0].min(delta);
        range[1] = range[1].max(delta + 1);
    }
    range
}

/// Fit each hue's upper-hull gamma: the smallest that keeps every test point on its approximate boundary outside the
/// display's peak.
#[allow(clippy::too_many_arguments)] // Mirrors the reference's table construction step by step.
fn upper_hull_gammas(
    hues: &[f32],
    cusps: &mut [[f32; 3]],
    peak_luminance: f32,
    limit_j_max: f32,
    mid_j: f32,
    focus_distance: f32,
    lower_hull_gamma_inverse: f32,
    output: &JmhParameters,
) {
    let luminance_limit = peak_luminance / REFERENCE_LUMINANCE;
    for index in BASE_INDEX..UPPER_WRAP_INDEX {
        let hue = hues[index];
        let cusp = [cusps[index][0], cusps[index][1]];
        let analytical_threshold = lerp(cusp[0], limit_j_max, FOCUS_GAIN_BLEND);
        let focus = focus_j(cusp[0], mid_j, limit_j_max);
        let tests = GAMMA_TEST_POSITIONS.map(|position| {
            let test_j = lerp(cusp[0], limit_j_max, position);
            let slope_gain = focus_gain(test_j, analytical_threshold, limit_j_max, focus_distance);
            let j_intersect_source = solve_j_intersect(test_j, cusp[1], focus, limit_j_max, slope_gain);
            let slope = compression_vector_slope(j_intersect_source, focus, limit_j_max, slope_gain);
            let j_intersect_cusp = solve_j_intersect(cusp[0], cusp[1], focus, limit_j_max, slope_gain);
            (j_intersect_source, slope, j_intersect_cusp)
        });
        let fits = |gamma: f32| {
            tests.iter().all(|(j_intersect_source, slope, j_intersect_cusp)| {
                let m = gamut_boundary_intersection(
                    cusp,
                    limit_j_max,
                    1.0 / gamma,
                    lower_hull_gamma_inverse,
                    *j_intersect_source,
                    *slope,
                    *j_intersect_cusp,
                );
                let j = j_intersect_source + (slope * m);
                jmh_to_rgb([j, m, hue], output).iter().any(|channel| *channel > luminance_limit)
            })
        };

        let mut low = GAMMA_MINIMUM;
        let mut high = low + GAMMA_SEARCH_STEP;
        while !fits(high) && high < GAMMA_MAXIMUM {
            low = high;
            high += GAMMA_SEARCH_STEP;
        }
        while high - low > GAMMA_ACCURACY {
            let test = (high + low) / 2.0;
            if fits(test) {
                high = test;
            } else {
                low = test;
            }
        }
        cusps[index][2] = 1.0 / high;
    }
    cusps[0][2] = cusps[LAST_NOMINAL_INDEX][2];
    cusps[UPPER_WRAP_INDEX][2] = cusps[BASE_INDEX][2];
    cusps[UPPER_WRAP_INDEX + 1][2] = cusps[BASE_INDEX + 1][2];
}

// ---------------------------------------------------------------------------------------------------------------------
// Matrices
// ---------------------------------------------------------------------------------------------------------------------

/// A row-major 3×3 matrix applied to column vectors, in the reference's `f32` precision.
type Matrix = [f32; 9];

/// A row-major 3×3 matrix in `f64`, for primaries-derived matrices and inverses.
type Matrix64 = [[f64; 3]; 3];

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (t * (b - a))
}

fn mul(matrix: &Matrix, vector: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|row| {
        (vector[0] * matrix[row * 3]) + (vector[1] * matrix[(row * 3) + 1]) + (vector[2] * matrix[(row * 3) + 2])
    })
}

fn mul_matrices(a: &Matrix, b: &Matrix) -> Matrix {
    std::array::from_fn(|index| {
        let (row, column) = (index / 3, index % 3);
        (a[row * 3] * b[column]) + (a[(row * 3) + 1] * b[3 + column]) + (a[(row * 3) + 2] * b[6 + column])
    })
}

fn diagonal(scale: [f32; 3]) -> Matrix {
    [scale[0], 0.0, 0.0, 0.0, scale[1], 0.0, 0.0, 0.0, scale[2]]
}

/// Invert through `f64`, as the reference does.
fn invert(matrix: &Matrix) -> Matrix {
    let wide: Matrix64 = std::array::from_fn(|row| std::array::from_fn(|column| f64::from(matrix[(row * 3) + column])));
    to_matrix(inverse64(wide))
}

/// The matrix's columns as `vec4f`s for a WGSL `mat3x3f`.
fn columns(matrix: &Matrix) -> [[f32; 4]; 3] {
    std::array::from_fn(|column| [matrix[column], matrix[3 + column], matrix[6 + column], 0.0])
}

fn to_matrix(matrix: Matrix64) -> Matrix {
    std::array::from_fn(|index| matrix[index / 3][index % 3] as f32)
}

fn mul64(a: Matrix64, b: Matrix64) -> Matrix64 {
    std::array::from_fn(|row| std::array::from_fn(|column| (0..3).map(|k| a[row][k] * b[k][column]).sum()))
}

fn mul_vector64(matrix: Matrix64, vector: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|row| (0..3).map(|k| matrix[row][k] * vector[k]).sum())
}

fn inverse64(m: Matrix64) -> Matrix64 {
    let cofactor = |r0: usize, r1: usize, c0: usize, c1: usize| (m[r0][c0] * m[r1][c1]) - (m[r0][c1] * m[r1][c0]);
    let adjugate = [
        [cofactor(1, 2, 1, 2), -cofactor(0, 2, 1, 2), cofactor(0, 1, 1, 2)],
        [-cofactor(1, 2, 0, 2), cofactor(0, 2, 0, 2), -cofactor(0, 1, 0, 2)],
        [cofactor(1, 2, 0, 1), -cofactor(0, 2, 0, 1), cofactor(0, 1, 0, 1)],
    ];
    let determinant = (m[0][0] * adjugate[0][0]) + (m[0][1] * adjugate[1][0]) + (m[0][2] * adjugate[2][0]);
    adjugate.map(|row| row.map(|value| value / determinant))
}

/// XYZ (Y one) of an xy chromaticity.
fn xy_to_xyz(xy: [f64; 2]) -> [f64; 3] {
    [xy[0] / xy[1], 1.0, (1.0 - xy[0] - xy[1]) / xy[1]]
}

/// The normalized primary matrix: RGB to XYZ with the white point at Y one, without adaptation.
fn rgb_to_xyz(primaries: &Primaries) -> Matrix64 {
    let [red, green, blue] = [primaries.red, primaries.green, primaries.blue].map(xy_to_xyz);
    let unscaled = [[red[0], green[0], blue[0]], [red[1], green[1], blue[1]], [red[2], green[2], blue[2]]];
    let scale = mul_vector64(inverse64(unscaled), xy_to_xyz(primaries.white));
    unscaled.map(|row| [row[0] * scale[0], row[1] * scale[1], row[2] * scale[2]])
}

/// RGB in `source` to RGB in `destination`, without white point adaptation.
fn rgb_to_rgb(source: &Primaries, destination: &Primaries) -> Matrix64 {
    mul64(inverse64(rgb_to_xyz(destination)), rgb_to_xyz(source))
}

/// Bradford chromatic adaptation of XYZ from one white point to another.
fn bradford(source_white: [f64; 2], destination_white: [f64; 2]) -> Matrix64 {
    let source = mul_vector64(BRADFORD, xy_to_xyz(source_white));
    let destination = mul_vector64(BRADFORD, xy_to_xyz(destination_white));
    let scale = [
        [destination[0] / source[0], 0.0, 0.0],
        [0.0, destination[1] / source[1], 0.0],
        [0.0, 0.0, destination[2] / source[2]],
    ];
    mul64(inverse64(BRADFORD), mul64(scale, BRADFORD))
}

#[cfg(test)]
#[path = "aces2.test.rs"]
mod tests;
