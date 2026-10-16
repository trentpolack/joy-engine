// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Bloom parity with luma.gl's multiscale bloom, the pipeline the TypeScript engine runs. HDR patches drawn by the
//! triangle renderer pass through the engine's display processing and are compared with a CPU transcription of
//! luma.gl's fragment path (thresholded tent extraction, tent downsamples, paired-tap Gaussians, bicubic
//! reconstruction, starburst, and the additive composite); the lens streaks, energy, and spread are also checked
//! directly. Skips without a GPU adapter or an HDR scene format; native only (browsers cannot block on readback).
#![cfg(not(target_arch = "wasm32"))]

use glam::Vec2;
use joy_engine::core::PostProcessingConfig;
use joy_engine::core::post_processing::{BloomQuality, ToneMapper};
use joy_engine::rendering::{
    BackendPreference, CAPTURE_FORMAT, ColorVertex, GpuContext, OffscreenTarget, TopDownCamera, TriangleFrame,
    TriangleRenderer, TriangleRendererOptions, top_down_orthographic,
};

/// Passes vertex colors through unchanged, so patches carry exact HDR values into the scene target.
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

/// Largest per-channel difference, in 8-bit steps, between the engine and the luma.gl transcription. Half-float
/// intermediates and hardware bilinear weights account for it.
const TOLERANCE: i32 = 2;

/// Pyramid level sizes relative to the drawing buffer (luma.gl's `BLOOM_PYRAMID_LEVELS`).
const LEVEL_SCALES: [f64; 5] = [0.5, 0.25, 0.125, 0.0625, 0.03125];

/// The TypeScript renderer's kernel scale: a radius of 0.025 at 720 pixels is eight texels.
const BLOOM_RADIUS_SCALE: f32 = 8.0 / (720.0 * 0.025);

const LUMINANCE: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// An axis-aligned rectangle of linear HDR color, in pixels.
#[derive(Clone, Copy, Debug)]
struct Patch {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    color: [f32; 3],
}

/// A frame to render: drawing size, background, and patches. Colors are exact in half precision.
#[derive(Clone, Debug)]
struct Scene {
    width: u32,
    height: u32,
    background: [f32; 3],
    patches: Vec<Patch>,
}

impl Scene {
    /// The linear scene color the triangle renderer draws.
    fn image(&self) -> Image {
        let mut image = Image::filled(self.width as usize, self.height as usize, rgba(self.background));
        for patch in &self.patches {
            for y in patch.y..patch.y + patch.height {
                for x in patch.x..patch.x + patch.width {
                    image.texels[(y * self.width + x) as usize] = rgba(patch.color);
                }
            }
        }
        image
    }
}

fn rgba(color: [f32; 3]) -> [f32; 4] {
    [color[0], color[1], color[2], 1.0]
}

fn gpu() -> Option<GpuContext> {
    let gpu = match pollster::block_on(GpuContext::headless(BackendPreference::Auto)) {
        Ok(gpu) => gpu,
        Err(error) => {
            eprintln!("skipping GPU test: {error}");
            return None;
        }
    };
    if !gpu.capabilities().hdr {
        eprintln!("skipping bloom test: the adapter has no HDR scene format");
        return None;
    }
    Some(gpu)
}

/// Display settings that leave only bloom: linear output at unit exposure with every other effect off.
fn bloom_config() -> PostProcessingConfig {
    PostProcessingConfig {
        tone_mapper: ToneMapper::None,
        antialias_strength: 0.0,
        bloom_strength: 1.0,
        bloom_threshold: 1.0,
        bloom_knee: 0.5,
        ..PostProcessingConfig::default()
    }
}

/// Render a scene through the engine's display processing and return RGBA8 pixels.
fn render(gpu: &GpuContext, scene: &Scene, config: &PostProcessingConfig) -> Vec<u8> {
    let target = OffscreenTarget::new(gpu, scene.width, scene.height).expect("capture target");
    let options =
        TriangleRendererOptions { shader: SHADER.to_owned(), uniform_size: 80, post_processing: Some(config.clone()) };
    let mut renderer = TriangleRenderer::new(gpu, CAPTURE_FORMAT, options).expect("renderer");
    renderer.resize(scene.width, scene.height, 1.0);
    let (width, height) = (scene.width as f32, scene.height as f32);
    let camera = TopDownCamera { height: 10.0, near: 1.0, far: 20.0 };
    let matrix = top_down_orthographic(width, height, Vec2::new(width * 0.5, height * 0.5), camera).unwrap();
    let mut uniforms = matrix.to_cols_array().to_vec();
    uniforms.extend([0.0; 4]);

    let mut vertices = Vec::new();
    for patch in &scene.patches {
        let (left, top) = (patch.x as f32, patch.y as f32);
        let (right, bottom) = (left + patch.width as f32, top + patch.height as f32);
        let color = rgba(patch.color);
        for (x, y) in [(left, top), (right, top), (right, bottom), (left, top), (right, bottom), (left, bottom)] {
            vertices.push(ColorVertex { position: [x, y, 0.0], color });
        }
    }
    let frame = TriangleFrame {
        opaque: &vertices,
        transparent: &[],
        uniforms: bytemuck::cast_slice(&uniforms),
        clear_color: rgba(scene.background),
        time_seconds: 0.0,
    };
    renderer.render(target.view(), &frame).expect("frame renders");
    target.read_rgba(gpu).expect("readback")
}

/// A float RGBA image sampled like a clamp-to-edge, linearly filtered texture.
#[derive(Clone, Debug)]
struct Image {
    width: usize,
    height: usize,
    texels: Vec<[f32; 4]>,
}

impl Image {
    fn filled(width: usize, height: usize, value: [f32; 4]) -> Self {
        Self { width, height, texels: vec![value; width * height] }
    }

    fn from_fn(width: usize, height: usize, texel: impl Fn(usize, usize) -> [f32; 4]) -> Self {
        let texels = (0..height).flat_map(|y| (0..width).map(move |x| (x, y))).map(|(x, y)| texel(x, y)).collect();
        Self { width, height, texels }
    }

    fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// `textureLoad` with clamped coordinates.
    fn load(&self, x: i64, y: i64) -> [f32; 4] {
        let x = x.clamp(0, self.width as i64 - 1) as usize;
        let y = y.clamp(0, self.height as i64 - 1) as usize;
        self.texels[(y * self.width) + x]
    }

    /// Bilinear sample at a normalized coordinate.
    fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let x = (u * self.width as f32) - 0.5;
        let y = (v * self.height as f32) - 0.5;
        let (left, top) = (x.floor(), y.floor());
        let (fx, fy) = (x - left, y - top);
        let (left, top) = (left as i64, top as i64);
        let row = |y: i64| lerp(self.load(left, y), self.load(left + 1, y), fx);
        lerp(row(top), row(top + 1), fy)
    }
}

fn lerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    std::array::from_fn(|index| a[index] + ((b[index] - a[index]) * t))
}

fn add(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    std::array::from_fn(|index| a[index] + b[index])
}

fn scale(a: [f32; 4], factor: f32) -> [f32; 4] {
    a.map(|value| value * factor)
}

fn level_size(size: usize, scale: f64) -> usize {
    ((size as f64 * scale).round() as usize).max(1)
}

/// luma.gl's `calculateTextureScale`.
fn texture_scale(source: (usize, usize), output: (usize, usize)) -> [f32; 2] {
    let source_aspect = source.0 as f64 / source.1 as f64;
    let output_aspect = output.0 as f64 / output.1 as f64;
    if output_aspect > source_aspect {
        [1.0, (output_aspect / source_aspect) as f32]
    } else {
        [(source_aspect / output_aspect) as f32, 1.0]
    }
}

/// An output texel center mapped to its source coordinate through luma.gl's texture transform.
fn texture_uv(x: usize, y: usize, output: (usize, usize), transform: [f32; 2]) -> (f32, f32) {
    let u = ((((x as f32) + 0.5) / output.0 as f32) - 0.5) / transform[0] + 0.5;
    let v = ((((y as f32) + 0.5) / output.1 as f32) - 0.5) / transform[1] + 0.5;
    (u, v)
}

/// luma.gl's `bloomExtract_applyThreshold` with the TypeScript renderer's soft-knee conversion.
fn threshold(color: [f32; 4], config: &PostProcessingConfig) -> [f32; 4] {
    let luminance = (color[0] * LUMINANCE[0]) + (color[1] * LUMINANCE[1]) + (color[2] * LUMINANCE[2]);
    let soft_knee = config.bloom_knee / config.bloom_threshold.max(0.00001);
    let threshold = config.bloom_threshold / config.exposure.max(0.0001);
    let knee = (threshold * soft_knee).max(0.00001);
    let soft = ((luminance - threshold + knee) / (2.0 * knee)).clamp(0.0, 1.0);
    let contribution = (luminance - threshold).max(0.0).max(soft * soft * knee) / luminance.max(0.00001);
    scale(color, contribution)
}

/// luma.gl's footprint-adaptive tent (`bloomExtract_sampleColor` / `bloomDownsample_sampleColor`).
fn tent(source: &Image, output: (usize, usize), extract: Option<&PostProcessingConfig>) -> Image {
    let transform = texture_scale(source.size(), output);
    // The screen-space derivative of the source coordinate is one output texel through the transform.
    let footprint = [
        (source.width as f32 / (output.0 as f32 * transform[0])).max(1.0),
        (source.height as f32 / (output.1 as f32 * transform[1])).max(1.0),
    ];
    let radius = footprint.map(|value| value.max(2.0));
    Image::from_fn(output.0, output.1, |x, y| {
        let (u, v) = texture_uv(x, y, output, transform);
        let center = [(u * source.width as f32) - 0.5, (v * source.height as f32) - 0.5];
        let (mut color, mut total) = ([0.0; 4], 0.0);
        for source_y in ((center[1] - radius[1]).floor() as i64 + 1)..=((center[1] + radius[1]).ceil() as i64 - 1) {
            let weight_y = (1.0 - ((source_y as f32 - center[1]).abs() / radius[1])).max(0.0);
            for source_x in ((center[0] - radius[0]).floor() as i64 + 1)..=((center[0] + radius[0]).ceil() as i64 - 1) {
                let weight_x = (1.0 - ((source_x as f32 - center[0]).abs() / radius[0])).max(0.0);
                let texel = source.load(source_x, source_y);
                let texel = extract.map_or(texel, |config| threshold(texel, config));
                color = add(color, scale(texel, weight_x * weight_y));
                total += weight_x * weight_y;
            }
        }
        scale(color, 1.0 / total.max(0.00001))
    })
}

/// luma.gl's `bloomBlur_sampleColor`: sigma radius/3 with each pair of taps folded into one linear fetch.
fn gaussian(source: &Image, delta: [f32; 2], radius: f32) -> Image {
    let radius = radius.clamp(0.0, 24.0);
    let weight = |offset: f32, sigma: f32| (-0.5 * (offset / sigma) * (offset / sigma)).exp();
    Image::from_fn(source.width, source.height, |x, y| {
        let (u, v) = texture_uv(x, y, source.size(), [1.0, 1.0]);
        let center = source.sample(u, v);
        if radius <= 0.0 {
            return center;
        }
        let step = [delta[0] / source.width as f32, delta[1] / source.height as f32];
        let sigma = (radius / 3.0).max(0.00001);
        let (mut color, mut total) = (scale(center, weight(0.0, sigma)), weight(0.0, sigma));
        let mut first = 1.0;
        while first <= radius {
            let second = first + 1.0;
            let first_weight = weight(first, sigma);
            let second_weight = if second <= radius { weight(second, sigma) } else { 0.0 };
            let combined = first_weight + second_weight;
            let offset = ((first * first_weight) + (second * second_weight)) / combined.max(0.00001);
            let positive = source.sample(u + (step[0] * offset), v + (step[1] * offset));
            let negative = source.sample(u - (step[0] * offset), v - (step[1] * offset));
            color = add(color, scale(add(positive, negative), combined));
            total += combined * 2.0;
            first += 2.0;
        }
        scale(color, 1.0 / total)
    })
}

/// luma.gl's `bloomUpsample_sampleBicubicGlow`: a cubic B-spline from four bilinear fetches.
fn bicubic(source: &Image, u: f32, v: f32) -> [f32; 4] {
    let dimensions = [source.width as f32, source.height as f32];
    let axis = |coordinate: f32, size: f32| {
        let position = (coordinate * size) - 0.5;
        let base = position.floor();
        let fraction = position - base;
        let complement = 1.0 - fraction;
        let (f2, f3) = (fraction * fraction, fraction * fraction * fraction);
        let first = complement * complement * complement / 6.0;
        let second = ((f3 * 3.0) - (f2 * 6.0) + 4.0) / 6.0;
        let third = ((-f3 * 3.0) + (f2 * 3.0) + (fraction * 3.0) + 1.0) / 6.0;
        let fourth = f3 / 6.0;
        let (near, far) = (first + second, third + fourth);
        ((base - 1.0 + (second / near) + 0.5) / size, (base + 1.0 + (fourth / far) + 0.5) / size, near, far)
    };
    let (u0, u1, wx0, wx1) = axis(u, dimensions[0]);
    let (v0, v1, wy0, wy1) = axis(v, dimensions[1]);
    let mut color = scale(source.sample(u0, v0), wx0 * wy0);
    color = add(color, scale(source.sample(u1, v0), wx1 * wy0));
    color = add(color, scale(source.sample(u0, v1), wx0 * wy1));
    add(color, scale(source.sample(u1, v1), wx1 * wy1))
}

/// luma.gl's starburst with the TypeScript renderer's two rays: a horizontal average of the glow.
fn starburst(glow: &Image, length: f32, intensity: f32) -> Image {
    Image::from_fn(glow.width, glow.height, |x, y| {
        let (u, v) = texture_uv(x, y, glow.size(), [1.0, 1.0]);
        let (mut color, mut total) = ([0.0; 4], 0.0);
        for index in 1..=8 {
            let distance = index as f32 / 8.0;
            let offset = length * distance / glow.width as f32;
            let weight = (-distance * 2.4).exp();
            let positive = glow.sample((u + offset).clamp(0.0, 1.0), v);
            let negative = glow.sample((u - offset).clamp(0.0, 1.0), v);
            color = add(color, scale(add(positive, negative), weight));
            total += weight * 2.0;
        }
        let streak = scale(color, intensity / total);
        [streak[0], streak[1], streak[2], 1.0]
    })
}

/// The reconstructed glow and streaks (half resolution) for a scene under `config`, through luma.gl's fragment path.
fn reference_bloom(scene: &Image, config: &PostProcessingConfig) -> (Image, Option<Image>) {
    let shortest_axis = scene.width.min(scene.height) as f32;
    let radius = (config.bloom_radius * shortest_axis * BLOOM_RADIUS_SCALE).min(24.0);
    let ratio = config.bloom_anamorphic.clamp(-1.0, 1.0);
    let anamorphic_radius = radius.min(24.0 / (1.0 + ratio.abs()));
    let horizontal = anamorphic_radius * (1.0 + ratio.max(0.0));
    let vertical = anamorphic_radius * (1.0 + (-ratio).max(0.0));

    let mut extracted: Vec<Image> = Vec::new();
    let mut blurred = Vec::new();
    for scale in LEVEL_SCALES.iter().take(pyramid_levels(config.bloom_quality)) {
        let size = (level_size(scene.width, *scale), level_size(scene.height, *scale));
        let extraction = match extracted.last() {
            None => tent(scene, size, Some(config)),
            Some(previous) => tent(previous, size, None),
        };
        blurred.push(gaussian(&gaussian(&extraction, [1.0, 0.0], horizontal), [0.0, 1.0], vertical));
        extracted.push(extraction);
    }
    let mut glow = blurred.pop().expect("at least two levels");
    while let Some(higher) = blurred.pop() {
        let transform = texture_scale(glow.size(), higher.size());
        let scatter = config.bloom_scatter.clamp(0.0, 1.0);
        glow = Image::from_fn(higher.width, higher.height, |x, y| {
            let (u, v) = texture_uv(x, y, higher.size(), transform);
            lerp(higher.sample(u, v), bicubic(&glow, u, v), scatter)
        });
    }
    let streaks = (config.bloom_lens_flare > 0.0).then(|| {
        let length = (config.bloom_radius * shortest_axis * 4.0).clamp(0.0, 256.0);
        starburst(&glow, length, config.bloom_lens_flare)
    });
    (glow, streaks)
}

/// Linear display color for every pixel: scene plus bloom, exposed, clamped (tone mapper `None`, neutral grade).
fn reference_display(scene: &Image, config: &PostProcessingConfig) -> Vec<[f32; 3]> {
    let (glow, streaks) = reference_bloom(scene, config);
    (0..scene.height)
        .flat_map(|y| (0..scene.width).map(move |x| (x, y)))
        .map(|(x, y)| {
            let (u, v) = texture_uv(x, y, scene.size(), [1.0, 1.0]);
            let mut bloom = glow.sample(u, v);
            if let Some(streaks) = &streaks {
                bloom = add(bloom, streaks.sample(u, v));
            }
            let color = add(scene.texels[(y * scene.width) + x], scale(bloom, config.bloom_strength));
            std::array::from_fn(|channel| (color[channel] * config.exposure).clamp(0.0, 1.0))
        })
        .collect()
}

/// luma.gl's `BLOOM_QUALITY_LEVELS`.
fn pyramid_levels(quality: BloomQuality) -> usize {
    match quality {
        BloomQuality::Low => 2,
        BloomQuality::Medium => 3,
        BloomQuality::High => 4,
        BloomQuality::Ultra => 5,
    }
}

/// Largest per-channel difference between engine pixels and the reference, with its location.
fn largest_difference(pixels: &[u8], reference: &[[f32; 3]], width: usize) -> (i32, usize, usize) {
    let mut worst = (0, 0, 0);
    for (index, expected) in reference.iter().enumerate() {
        for channel in 0..3 {
            let expected = (expected[channel] * 255.0).round() as i32;
            let difference = (i32::from(pixels[(index * 4) + channel]) - expected).abs();
            if difference > worst.0 {
                worst = (difference, index % width, index / width);
            }
        }
    }
    worst
}

/// A bright, saturated highlight above threshold, a near-threshold patch in the soft knee, and a dim background.
fn highlight_scene(width: u32, height: u32) -> Scene {
    Scene {
        width,
        height,
        background: [0.015625, 0.015625, 0.03125],
        patches: vec![
            Patch { x: width / 3, y: height / 2 - 2, width: 4, height: 4, color: [6.0, 3.0, 1.5] },
            Patch { x: (width * 2) / 3, y: height / 3, width: 12, height: 8, color: [1.125, 1.0, 0.875] },
        ],
    }
}

#[test]
fn bloom_matches_lumas_fragment_pipeline() {
    let Some(gpu) = gpu() else {
        return;
    };
    let cases = [
        // Defaults at an even size: every level and the aspect correction are exact.
        (256, 128, PostProcessingConfig { bloom_radius: 0.03, ..bloom_config() }),
        // Low quality with a wide anamorphic kernel.
        (
            256,
            128,
            PostProcessingConfig {
                bloom_quality: BloomQuality::Low,
                bloom_radius: 0.06,
                bloom_anamorphic: 0.5,
                ..bloom_config()
            },
        ),
        // Ultra quality, high scatter, and streaks.
        (
            256,
            128,
            PostProcessingConfig {
                bloom_quality: BloomQuality::Ultra,
                bloom_radius: 0.015,
                bloom_scatter: 0.8,
                bloom_lens_flare: 0.75,
                ..bloom_config()
            },
        ),
        // SPACE SCUFFLES' bloom at an odd size, where level rounding and the aspect correction matter.
        (
            250,
            130,
            PostProcessingConfig {
                bloom_quality: BloomQuality::Medium,
                bloom_radius: 0.03,
                bloom_anamorphic: 0.25,
                bloom_scatter: 0.0,
                bloom_lens_flare: 0.5,
                bloom_strength: 2.0,
                bloom_threshold: 0.6,
                bloom_knee: 0.44,
                exposure: 0.8,
                ..bloom_config()
            },
        ),
        // A zero radius leaves the Gaussians as copies.
        (256, 128, PostProcessingConfig { bloom_radius: 0.0, bloom_quality: BloomQuality::High, ..bloom_config() }),
    ];
    for (width, height, config) in cases {
        let scene = highlight_scene(width, height);
        let pixels = render(&gpu, &scene, &config);
        let reference = reference_display(&scene.image(), &config);
        let (difference, x, y) = largest_difference(&pixels, &reference, width as usize);
        assert!(difference <= TOLERANCE, "{width}x{height} {config:?}: off by {difference} at ({x}, {y})");
    }
}

#[test]
fn lens_flare_adds_horizontal_streaks_only() {
    let Some(gpu) = gpu() else {
        return;
    };
    let (width, height) = (320, 240);
    let scene = Scene {
        width,
        height,
        background: [0.0; 3],
        patches: vec![Patch { x: 158, y: 118, width: 4, height: 4, color: [64.0, 64.0, 64.0] }],
    };
    let red = |pixels: &[u8], x: u32, y: u32| i32::from(pixels[((y * width + x) * 4) as usize]);
    // Mean of both sides of the highlight along one axis, 36 pixels from its center (inside the 48-pixel rays).
    let horizontal = |pixels: &[u8]| (red(pixels, 160 - 36, 120) + red(pixels, 160 + 36, 120)) / 2;
    let vertical = |pixels: &[u8]| (red(pixels, 160, 120 - 36) + red(pixels, 160, 120 + 36)) / 2;

    let plain = render(&gpu, &scene, &bloom_config());
    let streaked = render(&gpu, &scene, &PostProcessingConfig { bloom_lens_flare: 1.0, ..bloom_config() });
    // Without streaks the halo is round.
    assert!((horizontal(&plain) - vertical(&plain)).abs() <= 2, "{} {}", horizontal(&plain), vertical(&plain));
    let horizontal_gain = horizontal(&streaked) - horizontal(&plain);
    let vertical_gain = vertical(&streaked) - vertical(&plain);
    assert!(
        horizontal_gain >= 20 && horizontal_gain >= vertical_gain * 4,
        "streaks brighten {horizontal_gain} horizontally and {vertical_gain} vertically"
    );
    // Beyond the rays' reach, the streaks fade with the glow.
    let far = |pixels: &[u8]| red(pixels, 160 + 120, 120);
    assert!(far(&streaked) - far(&plain) <= 2, "{} {}", far(&streaked), far(&plain));
}

#[test]
fn bloom_energy_and_spread_follow_quality_and_radius() {
    let Some(gpu) = gpu() else {
        return;
    };
    // A white patch below display white with a low threshold, so neither the patch nor its glow clips.
    let (width, height) = (320_usize, 240_usize);
    let scene = Scene {
        width: width as u32,
        height: height as u32,
        background: [0.0; 3],
        patches: vec![Patch { x: 157, y: 117, width: 6, height: 6, color: [0.5, 0.5, 0.5] }],
    };
    let source = scene.image();
    let base = PostProcessingConfig { bloom_threshold: 0.1, bloom_knee: 0.05, bloom_strength: 2.0, ..bloom_config() };
    let unbloomed = render(&gpu, &scene, &PostProcessingConfig { bloom_strength: 0.0, ..base.clone() });
    // Thresholded patch energy: the luminance above threshold, carried in each channel.
    let extracted = 36.0 * (0.5 - 0.1) * f64::from(base.bloom_strength);

    // Glow energy (sum of added red) and spread (RMS distance from the patch center) over an image whose texels each
    // cover `area` display pixels.
    let measure = |size: (usize, usize), area: f64, glow: &dyn Fn(usize) -> f64| {
        let (mut energy, mut moment) = (0.0, 0.0);
        let pixels = [width as f64 / size.0 as f64, height as f64 / size.1 as f64];
        for index in 0..size.0 * size.1 {
            let x = ((((index % size.0) as f64) + 0.5) * pixels[0]) - 0.5;
            let y = ((((index / size.0) as f64) + 0.5) * pixels[1]) - 0.5;
            energy += glow(index) * area;
            moment += glow(index) * area * ((x - 159.5).powi(2) + (y - 119.5).powi(2));
        }
        (energy, (moment / energy).sqrt())
    };
    let quantized = |value: f32| f64::from((value * 255.0).round()) / 255.0;

    let mut spreads = Vec::new();
    for radius in [0.01, 0.03] {
        let mut previous_spread = 0.0;
        for quality in [BloomQuality::Low, BloomQuality::Medium, BloomQuality::High, BloomQuality::Ultra] {
            let config = PostProcessingConfig { bloom_radius: radius, bloom_quality: quality, ..base.clone() };
            let pixels = render(&gpu, &scene, &config);
            let reference = reference_display(&source, &config);

            // luma.gl's filters are all normalized, so the reconstructed (half-resolution) glow carries the extracted
            // energy. Ultra's widest level (10 by 8 texels) reaches the border, where clamp-to-edge adds about 2%.
            let (glow, _) = reference_bloom(&source, &config);
            let area = (width * height) as f64 / (glow.width * glow.height) as f64;
            let strength = f64::from(config.bloom_strength);
            let (exact_energy, exact_spread) =
                measure(glow.size(), area, &|index| f64::from(glow.texels[index][0]) * strength);
            assert!(
                (exact_energy / extracted - 1.0).abs() < 0.03,
                "{quality:?} {radius}: {exact_energy} of {extracted}"
            );

            // The engine matches the transcription once both are quantized to the display's eight bits.
            let (reference_energy, reference_spread) = measure((width, height), 1.0, &|index| {
                quantized(reference[index][0]) - quantized(source.texels[index][0])
            });
            let (energy, spread) = measure((width, height), 1.0, &|index| {
                f64::from(i32::from(pixels[index * 4]) - i32::from(unbloomed[index * 4])) / 255.0
            });
            assert!(
                (energy / reference_energy - 1.0).abs() < 0.02 && (spread / reference_spread - 1.0).abs() < 0.02,
                "{quality:?} {radius}: engine energy {energy} spread {spread}, luma.gl {reference_energy} \
                 {reference_spread}"
            );

            // Each level carries the glow further out.
            assert!(
                exact_spread > previous_spread,
                "{quality:?} {radius}: spread {exact_spread} after {previous_spread}"
            );
            previous_spread = exact_spread;
            spreads.push(exact_spread);
        }
    }
    // A wider radius spreads every quality further. (Eight-bit pixels lose the faint outer levels, so the spread
    // ordering is checked on the transcription the engine was just shown to match.)
    for (narrow, wide) in spreads[..4].iter().zip(&spreads[4..]) {
        assert!(wide > narrow, "radius 0.03 spread {wide} not wider than radius 0.01 spread {narrow}");
    }
}

#[test]
fn early_composite_matches_resolve_composite_away_from_the_edge_blur() {
    let Some(gpu) = gpu() else {
        return;
    };
    // The vignette edge blur reads the bloomed scene, so bloom composes before it; away from the edges the blur weight
    // is zero and the picture must match bloom composed in the resolve.
    let scene = highlight_scene(256, 128);
    let config = PostProcessingConfig { bloom_lens_flare: 0.5, ..bloom_config() };
    let resolved = render(&gpu, &scene, &config);
    let edged = render(&gpu, &scene, &PostProcessingConfig { vignette_blur: 1.0, vignette_radius: 0.6, ..config });
    let mut checked = 0;
    for y in 0..128_usize {
        for x in 0..256_usize {
            let offset = [(x as f32 + 0.5) / 256.0 - 0.5, (y as f32 + 0.5) / 128.0 - 0.5];
            // Aspect-corrected distance, as the edge-blur mask measures it.
            if ((offset[0] * 2.0).powi(2) + offset[1].powi(2)).sqrt() >= 0.55 {
                continue;
            }
            let index = ((y * 256) + x) * 4;
            for channel in 0..3 {
                let difference = (i32::from(resolved[index + channel]) - i32::from(edged[index + channel])).abs();
                assert!(difference <= 1, "({x}, {y}) differs by {difference}");
            }
            checked += 1;
        }
    }
    assert!(checked > 1000);
}
