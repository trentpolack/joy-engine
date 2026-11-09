// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Bloom on a real device: HDR patches drawn through the scene renderer's top-down view at unit ambient (which leaves
//! their colors exact) pass through the engine's display processing with no tone curve, so glow reads back as linear
//! light around them. Light below the threshold never glows, glow
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
    FrameInputs,
    GpuContext,
    OffscreenTarget,
    RenderScene,
    Renderer,
    RendererOptions,
    TopDownCamera,
    TopDownView,
};

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
    let options = RendererOptions {
        post_process: config.clone(),
        shadows: None,
        ..RendererOptions::default()
    };
    let mut renderer = Renderer::new(gpu, CAPTURE_FORMAT, options);
    renderer.resize(WIDTH, HEIGHT, 1.0);
    let size = Vec2::new(WIDTH as f32, HEIGHT as f32);
    let camera = TopDownCamera {
        height: 10.0,
        near: 1.0,
        far: 20.0,
    };
    let view = TopDownView::new(size, size * 0.5, camera).unwrap();

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
    // Unit ambient and no other light leave the lit transient triangles at exactly their vertex color.
    let inputs = FrameInputs {
        opaque_triangles: &vertices,
        ambient: Some(1.0),
        clear_color: LinearColor::rgb(0.0, 0.0, 0.0),
        ..FrameInputs::default()
    };
    renderer.render(&RenderScene::new(), &view, &inputs, target.view()).expect("frame renders");
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
