// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Display output: what the resolve writes for standard and HDR surfaces. Bands of known linear scene light drawn by the
//! triangle renderer pass through the display transform into a half-float target, as an HDR surface would receive
//! them, and are read back as floats. Normally lit content must match standard output on every encoding, highlights
//! must rise into the display's headroom and never beyond it, extended linear (scRGB) output must carry the same image
//! as light scaled to the display's SDR white, ACES 2.0 must match OpenColorIO's reference, and the filmic curve must
//! keep middle gray. Skips without a GPU adapter or an HDR scene format; native only (browsers cannot block on
//! readback).
#![cfg(not(target_arch = "wasm32"))]

use joy_core::LinearColor;
use joy_core::color::{
    linear_to_srgb,
    srgb_to_linear,
};
use joy_core::math::Vec2;
use joy_renderer::PostProcessSettings;
use joy_renderer::post_process::ToneMapper;
use joy_renderer::{
    BackendPreference,
    CAPTURE_FORMAT,
    ColorVertex,
    DisplayOutput,
    GpuContext,
    OffscreenTarget,
    OutputEncoding,
    TopDownCamera,
    TriangleFrame,
    TriangleRenderer,
    TriangleRendererOptions,
    top_down_orthographic,
};

/// Passes vertex colors through unchanged, so bands carry exact linear light into the scene target.
const SHADER: &str = r"
struct Uniforms {
  viewProjection: mat4x4f,
  frame: vec4f,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexOutput {
  @builtin(position) position: vec4f,
  @location(0) color: vec4f,
};

@vertex fn vertexMain(@location(0) position: vec3f, @location(1) color: vec4f) -> VertexOutput {
  var output: VertexOutput;
  output.position = uniforms.viewProjection*vec4f(position, 1.0);
  output.color = color;
  return output;
}

@fragment fn fragmentMain(input: VertexOutput) -> @location(0) vec4f {
  return input.color;
}
";

/// Format extended-range surfaces present.
const EXTENDED_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Drawing size: one band per `BAND_WIDTH` columns. Rows of 64 half-float texels meet the copy alignment.
const WIDTH: u32 = 64;
const HEIGHT: u32 = 8;
const BAND_WIDTH: u32 = 16;

/// Gray scene light per band: a midtone, SDR white, a bright highlight, and a light far past any tone mapper's
/// shoulder. Exact in half precision.
const BANDS: [[f32; 3]; 4] = [[0.18; 3], [1.0; 3], [8.0; 3], [256.0; 3]];

/// Linear Rec.709 scene colors and the display-linear Rec.709 output OpenColorIO 2.6 computes for them with its
/// built-in "ACES 2.0 SDR 100 nits Rec.709" output transform (the same references `tone_mapping/aces2.rs` tests).
const ACES_REFERENCE: [([f32; 3], [f32; 3]); 4] = [
    ([0.18, 0.18, 0.18], [0.0999991, 0.0999994, 0.0999993]),
    ([1.0, 0.0, 0.0], [0.5540078, 0.0088254, 0.0033028]),
    ([0.05, 0.2, 0.8], [0.0255990, 0.1119426, 0.4629686]),
    ([10.0, 0.5, 0.1], [1.0, 0.4766966, 0.3737049]),
];

/// Headroom and SDR white level of the HDR displays under test.
const HEADROOM: f32 = 4.0;
const SDR_WHITE_LEVEL: f32 = 2.5;

/// Largest difference between encoded values that should agree: one 8-bit step plus half-float rounding.
const ENCODED_TOLERANCE: f32 = 1.5 / 255.0;

fn gpu() -> Option<GpuContext> {
    let gpu = match pollster::block_on(GpuContext::headless(BackendPreference::Auto)) {
        Ok(gpu) => gpu,
        Err(error) => {
            eprintln!("skipping GPU test: {error}");
            return None;
        }
    };
    if !gpu.capabilities().hdr {
        eprintln!("skipping display output test: the adapter has no HDR scene format");
        return None;
    }
    Some(gpu)
}

/// Display settings with only exposure and tone mapping, so each band reads back as one value.
fn settings(tone_mapper: ToneMapper) -> PostProcessSettings {
    PostProcessSettings {
        tone_mapper,
        ..PostProcessSettings::DEFAULT
    }
}

/// A triangle renderer for the bands, presenting into `format` for `display`.
fn band_renderer(
    gpu: &GpuContext,
    format: wgpu::TextureFormat,
    settings: &PostProcessSettings,
    display: DisplayOutput,
) -> TriangleRenderer {
    let options =
        TriangleRendererOptions { shader: SHADER.to_owned(), uniform_size: 80, post_process: Some(settings.clone()) };
    let mut renderer = TriangleRenderer::new(gpu, format, options).expect("renderer");
    renderer.resize(WIDTH, HEIGHT, 1.0);
    renderer.set_display_output(display);
    renderer
}

/// Draw the bands of `colors` into `output`.
fn draw_bands(renderer: &mut TriangleRenderer, output: &wgpu::TextureView, colors: &[[f32; 3]; 4]) {
    let (width, height) = (WIDTH as f32, HEIGHT as f32);
    let camera = TopDownCamera { height: 10.0, near: 1.0, far: 20.0 };
    let matrix = top_down_orthographic(width, height, Vec2::new(width * 0.5, height * 0.5), camera).unwrap();
    let mut uniforms = matrix.to_cols_array().to_vec();
    uniforms.extend([0.0; 4]);

    let mut vertices = Vec::new();
    for (index, light) in colors.iter().enumerate() {
        let (left, right) = ((index as u32 * BAND_WIDTH) as f32, ((index as u32 + 1) * BAND_WIDTH) as f32);
        let color = [light[0], light[1], light[2], 1.0];
        for (x, y) in [(left, 0.0), (right, 0.0), (right, height), (left, 0.0), (right, height), (left, height)] {
            vertices.push(ColorVertex { position: [x, y, 0.0], color });
        }
    }
    let frame = TriangleFrame {
        opaque: &vertices,
        transparent: &[],
        uniforms: bytemuck::cast_slice(&uniforms),
        clear_color: LinearColor::rgb(0.0, 0.0, 0.0),
    };
    renderer.render(output, &frame).expect("frame renders");
}

/// Each gray band's red channel as standard output stores it: encoded values from 0 to 1.
fn standard_bands(gpu: &GpuContext, settings: &PostProcessSettings) -> [f32; 4] {
    let target = OffscreenTarget::new(gpu, WIDTH, HEIGHT).expect("capture target");
    let mut renderer = band_renderer(gpu, CAPTURE_FORMAT, settings, DisplayOutput::STANDARD);
    draw_bands(&mut renderer, target.view(), &BANDS);
    let pixels = target.read_rgba(gpu).expect("readback");
    band_centers().map(|x| f32::from(pixels[((HEIGHT / 2 * WIDTH + x) * 4) as usize]) / 255.0)
}

/// Each gray band's red channel as an extended-range surface receives it.
fn extended_bands(gpu: &GpuContext, settings: &PostProcessSettings, display: DisplayOutput) -> [f32; 4] {
    extended_colors(gpu, settings, display, &BANDS).map(|color| color[0])
}

/// Each band of `colors` as an extended-range surface receives it, RGB.
fn extended_colors(
    gpu: &GpuContext,
    settings: &PostProcessSettings,
    display: DisplayOutput,
    colors: &[[f32; 3]; 4],
) -> [[f32; 3]; 4] {
    let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("extended output"),
        size: wgpu::Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: EXTENDED_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut renderer = band_renderer(gpu, EXTENDED_FORMAT, settings, display);
    draw_bands(&mut renderer, &view, colors);
    let texels = read_half_floats(gpu, &texture);
    band_centers().map(|x| {
        let index = ((HEIGHT / 2 * WIDTH + x) * 4) as usize;
        [texels[index], texels[index + 1], texels[index + 2]]
    })
}

/// The center column of each band.
fn band_centers() -> [u32; 4] {
    std::array::from_fn(|index| (index as u32 * BAND_WIDTH) + (BAND_WIDTH / 2))
}

/// Copy a half-float RGBA texture to the CPU as `f32` channels, top row first.
fn read_half_floats(gpu: &GpuContext, texture: &wgpu::Texture) -> Vec<f32> {
    let row_bytes = WIDTH * 8;
    let buffer = gpu.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("extended readback"),
        size: u64::from(row_bytes * HEIGHT),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device().create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes),
                rows_per_image: Some(HEIGHT),
            },
        },
        wgpu::Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: 1 },
    );
    gpu.queue().submit([encoder.finish()]);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| result.expect("readback maps"));
    gpu.device().poll(wgpu::PollType::wait_indefinitely()).expect("readback completes");
    let mapped = slice.get_mapped_range().expect("mapped range");
    mapped.chunks_exact(2).map(|bits| half_to_f32(u16::from_le_bytes([bits[0], bits[1]]))).collect()
}

/// Decode an IEEE 754 half-precision value.
fn half_to_f32(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = i32::from((bits >> 10) & 0x1f);
    let mantissa = f32::from(bits & 0x3ff);
    match exponent {
        0 => sign * mantissa * 2.0_f32.powi(-24),
        31 => sign * f32::INFINITY,
        _ => sign * (1.0 + (mantissa / 1024.0)) * 2.0_f32.powi(exponent - 15),
    }
}

fn encode(linear: f32) -> f32 {
    linear_to_srgb(f64::from(linear)) as f32
}

fn decode(encoded: f32) -> f32 {
    srgb_to_linear(f64::from(encoded)) as f32
}

#[test]
fn hdr_output_keeps_midtones_and_lifts_highlights_into_the_headroom() {
    let Some(gpu) = gpu() else {
        return;
    };
    let tone_mapped = settings(ToneMapper::Agx);
    let standard = standard_bands(&gpu, &tone_mapped);
    let encoded =
        extended_bands(&gpu, &tone_mapped, DisplayOutput::extended(OutputEncoding::ExtendedSrgb, HEADROOM, 1.0));
    let peak = encode(HEADROOM);

    // The midtone is untouched; the brightest light rises well above SDR white but stays within the headroom.
    assert!((encoded[0] - standard[0]).abs() <= ENCODED_TOLERANCE, "midtone {encoded:?} vs SDR {standard:?}");
    assert!(encoded[3] > 1.2 && encoded[3] <= peak + 0.01, "highlight {encoded:?} against peak {peak}");
    assert!(encoded.windows(2).all(|pair| pair[0] < pair[1]), "brighter light stays brighter: {encoded:?}");

    // An extended surface on a display without headroom shows exactly the standard image.
    let no_headroom =
        extended_bands(&gpu, &tone_mapped, DisplayOutput::extended(OutputEncoding::ExtendedSrgb, 1.0, 1.0));
    for (band, (extended, standard)) in no_headroom.iter().zip(standard).enumerate() {
        assert!((extended - standard).abs() <= ENCODED_TOLERANCE, "band {band}: {no_headroom:?} vs SDR {standard}");
    }

    // Extended linear output is the same image as light, with SDR white at the display's SDR white level.
    let linear = extended_bands(
        &gpu,
        &tone_mapped,
        DisplayOutput::extended(OutputEncoding::ExtendedLinear, HEADROOM, SDR_WHITE_LEVEL),
    );
    for (band, (light, encoded)) in linear.iter().zip(encoded).enumerate() {
        let expected = decode(encoded) * SDR_WHITE_LEVEL;
        assert!((light - expected).abs() <= expected * 0.01 + 1e-3, "band {band}: {linear:?} vs {expected}");
    }
    assert!(linear[3] <= HEADROOM * SDR_WHITE_LEVEL * 1.01, "linear peak {linear:?}");

    // Without a tone mapper, scene light shows as it is up to the display's peak.
    let untouched = extended_bands(
        &gpu,
        &settings(ToneMapper::None),
        DisplayOutput::extended(OutputEncoding::ExtendedSrgb, HEADROOM, 1.0),
    );
    assert!((untouched[1] - 1.0).abs() <= ENCODED_TOLERANCE, "SDR white {untouched:?}");
    assert!((untouched[2] - peak).abs() <= 0.01 && (untouched[3] - peak).abs() <= 0.01, "clamped {untouched:?}");
}

#[test]
fn aces_matches_opencolorio_and_renders_for_the_display_peak() {
    let Some(gpu) = gpu() else {
        return;
    };
    let aces = settings(ToneMapper::Aces);

    // Standard output, read as linear light: within half a percent of the reference, plus half-float rounding.
    let colors = ACES_REFERENCE.map(|(input, _)| input);
    let standard = DisplayOutput::extended(OutputEncoding::ExtendedLinear, 1.0, 1.0);
    let rendered = extended_colors(&gpu, &aces, standard, &colors);
    for ((input, expected), output) in ACES_REFERENCE.iter().zip(rendered) {
        for (channel, (value, reference)) in output.iter().zip(expected).enumerate() {
            let tolerance = (reference * 0.005) + 2e-3;
            assert!((value - reference).abs() <= tolerance, "{input:?} channel {channel}: {output:?} vs {expected:?}");
        }
    }

    // With headroom, ACES renders for the brighter peak itself: brighter midtones and highlights within the headroom.
    let hdr = extended_bands(&gpu, &aces, DisplayOutput::extended(OutputEncoding::ExtendedLinear, HEADROOM, 1.0));
    let sdr = extended_bands(&gpu, &aces, standard);
    assert!(hdr[0] > sdr[0] * 1.05, "midtone rises with the peak: {hdr:?} vs {sdr:?}");
    assert!(hdr[3] > 2.0 && hdr[3] <= HEADROOM * 1.001, "highlights use the headroom: {hdr:?}");
    assert!(hdr.windows(2).all(|pair| pair[0] < pair[1]), "brighter light stays brighter: {hdr:?}");
}

#[test]
fn filmic_keeps_middle_gray_and_rolls_highlights_toward_white() {
    let Some(gpu) = gpu() else {
        return;
    };
    let standard = DisplayOutput::extended(OutputEncoding::ExtendedLinear, 1.0, 1.0);
    let bands = extended_bands(&gpu, &settings(ToneMapper::Filmic), standard);
    assert!((bands[0] - 0.18).abs() < 0.01, "middle gray: {bands:?}");
    assert!(bands[1] < 1.0 && bands[3] > 0.99, "shoulder approaches white: {bands:?}");
    assert!(bands.windows(2).all(|pair| pair[0] < pair[1]), "monotonic: {bands:?}");

    // More contrast steepens the linear section around middle gray.
    let contrasty = PostProcessSettings {
        filmic_contrast: 1.5,
        ..settings(ToneMapper::Filmic)
    };
    let steeper = extended_bands(&gpu, &contrasty, standard);
    assert!(steeper[1] > bands[1], "contrast lifts white: {steeper:?} vs {bands:?}");
}
