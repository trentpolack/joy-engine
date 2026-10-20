// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Bloom on a real device: HDR patches drawn by the triangle renderer pass through the engine's display processing
//! with no tone curve, so glow reads back as linear light around them. Light below the threshold never glows, glow
//! grows with strength, the radius sets how far it reaches, and every filtering quality produces a smooth halo. Skips
//! without a GPU adapter or an HDR scene format; native only (browsers cannot block on readback).
#![cfg(not(target_arch = "wasm32"))]

use joy_core::LinearColor;
use joy_core::color::srgb_to_linear;
use joy_core::math::Vec2;
use joy_renderer::PostProcessSettings;
use joy_renderer::post_process::{
    BloomQuality,
    ToneMapper,
};
use joy_renderer::{
    BackendPreference,
    CAPTURE_FORMAT,
    ColorVertex,
    GpuContext,
    OffscreenTarget,
    TopDownCamera,
    TriangleFrame,
    TriangleRenderer,
    TriangleRendererOptions,
    top_down_orthographic,
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

/// Drawing size; the patch sits at the center.
const WIDTH: u32 = 256;
const HEIGHT: u32 = 128;
const PATCH: u32 = 4;

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

/// Display settings that leave only bloom: linear output at unit exposure.
fn bloom(strength: f32, radius: f32, quality: BloomQuality) -> PostProcessSettings {
    PostProcessSettings {
        tone_mapper: ToneMapper::None,
        bloom_strength: strength,
        bloom_radius: radius,
        bloom_quality: quality,
        bloom_threshold: 1.0,
        bloom_knee: 0.5,
        ..PostProcessSettings::DEFAULT
    }
}

/// Render one centered patch of `light` and return each pixel's linear red channel, row by row.
fn render(gpu: &GpuContext, light: f32, config: &PostProcessSettings) -> Vec<f32> {
    let target = OffscreenTarget::new(gpu, WIDTH, HEIGHT).expect("capture target");
    let options = TriangleRendererOptions {
        shader: SHADER.to_owned(),
        uniform_size: 80,
        post_process: Some(config.clone()),
    };
    let mut renderer = TriangleRenderer::new(gpu, CAPTURE_FORMAT, options).expect("renderer");
    renderer.resize(WIDTH, HEIGHT, 1.0);
    let (width, height) = (WIDTH as f32, HEIGHT as f32);
    let camera = TopDownCamera {
        height: 10.0,
        near: 1.0,
        far: 20.0,
    };
    let matrix = top_down_orthographic(width, height, Vec2::new(width * 0.5, height * 0.5), camera).unwrap();
    let mut uniforms = matrix.to_cols_array().to_vec();
    uniforms.extend([0.0; 4]);

    let (left, top) = (((WIDTH - PATCH) / 2) as f32, ((HEIGHT - PATCH) / 2) as f32);
    let (right, bottom) = (left + PATCH as f32, top + PATCH as f32);
    let color = [light, light, light, 1.0];
    let vertices: Vec<ColorVertex> =
        [(left, top), (right, top), (right, bottom), (left, top), (right, bottom), (left, bottom)]
            .into_iter()
            .map(|(x, y)| ColorVertex {
                position: [x, y, 0.0],
                color,
            })
            .collect();
    let frame = TriangleFrame {
        opaque: &vertices,
        transparent: &[],
        uniforms: bytemuck::cast_slice(&uniforms),
        clear_color: LinearColor::rgb(0.0, 0.0, 0.0),
    };
    renderer.render(target.view(), &frame).expect("frame renders");
    let pixels = target.read_rgba(gpu).expect("readback");
    let (rgba_pixels, _) = pixels.as_chunks::<4>();
    rgba_pixels.iter().map(|pixel| srgb_to_linear(f64::from(pixel[0]) / 255.0) as f32).collect()
}

/// Linear light along the patch's row at `distance` pixels right of its edge.
fn glow_at(pixels: &[f32], distance: u32) -> f32 {
    let x = ((WIDTH + PATCH) / 2) + distance;
    pixels[((HEIGHT / 2) * WIDTH + x) as usize]
}

/// Sum of light outside the patch.
fn halo_energy(pixels: &[f32]) -> f32 {
    let (left, top) = ((WIDTH - PATCH) / 2, (HEIGHT - PATCH) / 2);
    let inside = |x: u32, y: u32| (left..left + PATCH).contains(&x) && (top..top + PATCH).contains(&y);
    let mut energy = 0.0;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            if !inside(x, y) {
                energy += pixels[(y * WIDTH + x) as usize];
            }
        }
    }
    energy
}

#[test]
fn bloom_glows_above_the_threshold_with_strength_and_reach() {
    let Some(gpu) = gpu() else {
        return;
    };

    // Light below the threshold (and its knee) never glows.
    let dim = render(&gpu, 0.25, &bloom(2.0, 0.05, BloomQuality::High));
    assert!(halo_energy(&dim) < 1e-3, "dim patch glowed: {}", halo_energy(&dim));

    // A bright patch glows, and more strength means more glow.
    let weak = render(&gpu, 32.0, &bloom(0.5, 0.05, BloomQuality::High));
    let strong = render(&gpu, 32.0, &bloom(1.0, 0.05, BloomQuality::High));
    let (weak_energy, strong_energy) = (halo_energy(&weak), halo_energy(&strong));
    assert!(weak_energy > 0.5, "bright patch glows: {weak_energy}");
    assert!(strong_energy > weak_energy * 1.6, "strength scales glow: {weak_energy} -> {strong_energy}");

    // Glow falls off with distance, and a longer radius carries it further.
    assert!(glow_at(&strong, 2) > glow_at(&strong, 12), "falloff");
    let far = render(&gpu, 32.0, &bloom(1.0, 0.25, BloomQuality::High));
    assert!(glow_at(&far, 24) > glow_at(&strong, 24) * 1.5, "reach: {} vs {}", glow_at(&far, 24), glow_at(&strong, 24));
}

#[test]
fn every_bloom_quality_produces_a_smooth_halo() {
    let Some(gpu) = gpu() else {
        return;
    };
    for quality in [BloomQuality::Low, BloomQuality::Medium, BloomQuality::High, BloomQuality::Ultra] {
        let pixels = render(&gpu, 32.0, &bloom(1.0, 0.1, quality));
        assert!(halo_energy(&pixels) > 0.5, "{quality:?} glows");
        // The halo decreases away from the patch without holes from sparse filter taps.
        let profile: Vec<f32> = (1..16).map(|distance| glow_at(&pixels, distance)).collect();
        let reversals = profile.windows(2).filter(|pair| pair[1] > pair[0] + (2.0 / 255.0)).count();
        assert_eq!(reversals, 0, "{quality:?} profile {profile:?}");
    }
}
