// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The arcade path through the scene renderer: a top-down view over a logical world (+Y down the screen), transient
//! opaque triangles at unit ambient that keep their exact colors and write depth, and blended transparency that tests
//! it. Skips without a GPU adapter; native only (browsers cannot block on readback).
#![cfg(not(target_arch = "wasm32"))]

use joy_core::LinearColor;
use joy_core::math::Vec2;
use joy_renderer::post_process::ToneMapper;
use joy_renderer::{
    BackendPreference,
    CAPTURE_FORMAT,
    ColorVertex,
    FrameInputs,
    GpuContext,
    OffscreenTarget,
    PostProcessSettings,
    RenderScene,
    Renderer,
    RendererOptions,
    TopDownCamera,
    TopDownView,
    add_circle,
    add_quad,
};

const WIDTH: u32 = 200;
const HEIGHT: u32 = 100;

fn gpu() -> Option<GpuContext> {
    match pollster::block_on(GpuContext::headless(BackendPreference::Auto)) {
        Ok(gpu) => Some(gpu),
        Err(error) => {
            eprintln!("skipping GPU test: {error}");
            None
        }
    }
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let index = ((y * WIDTH + x) * 4) as usize;
    [pixels[index], pixels[index + 1], pixels[index + 2], pixels[index + 3]]
}

#[test]
fn top_down_frames_draw_opaque_depth_then_blended_transparency() {
    let Some(gpu) = gpu() else {
        return;
    };
    let target = OffscreenTarget::new(&gpu, WIDTH, HEIGHT).unwrap();
    let options = RendererOptions {
        post_process: PostProcessSettings {
            tone_mapper: ToneMapper::None,
            ..PostProcessSettings::DEFAULT
        },
        shadows: None,
        ..RendererOptions::default()
    };
    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, options);
    renderer.resize(WIDTH, HEIGHT, 1.0);

    // Logical world: 200×100 units centered on (100, 50); the camera sits at z = 100.
    let camera = TopDownCamera {
        height: 100.0,
        near: 1.0,
        far: 201.0,
    };
    let view = TopDownView::new(Vec2::new(WIDTH as f32, HEIGHT as f32), Vec2::new(100.0, 50.0), camera).unwrap();

    // Opaque: a red background circle far away and a green square nearer the camera in the upper left.
    let mut opaque = Vec::new();
    add_circle(&mut opaque, 100.0, 50.0, 400.0, LinearColor::new(0.5, 0.0, 0.0, 1.0), 24, -50.0);
    add_quad(&mut opaque, 50.0, 30.0, 20.0, LinearColor::new(0.0, 0.5, 0.0, 1.0), std::f32::consts::FRAC_PI_4, 0.0);
    // Transparent: a half-white square on the right above the background, and a blue square *below* the green one,
    // which depth must hide. Supplied in no particular order: the renderer sorts transparency.
    let mut transparent: Vec<ColorVertex> = Vec::new();
    add_quad(
        &mut transparent,
        50.0,
        30.0,
        8.0,
        LinearColor::new(0.0, 0.0, 1.0, 1.0),
        std::f32::consts::FRAC_PI_4,
        -10.0,
    );
    add_quad(
        &mut transparent,
        150.0,
        50.0,
        30.0,
        LinearColor::new(1.0, 1.0, 1.0, 0.5),
        std::f32::consts::FRAC_PI_4,
        10.0,
    );

    // Unit ambient and no other light leave the lit transient triangles at exactly their vertex color.
    let inputs = FrameInputs {
        opaque_triangles: &opaque,
        transparent_triangles: &transparent,
        ambient: Some(1.0),
        clear_color: LinearColor::BLACK,
        ..FrameInputs::default()
    };
    renderer.render(&RenderScene::new(), &view, &inputs, target.view()).unwrap();
    let pixels = target.read_rgba(&gpu).unwrap();

    // Linear 0.5 encodes to about 188 in sRGB.
    let background = pixel(&pixels, 100, 10);
    assert!(background[0].abs_diff(188) <= 2 && background[1] < 5, "exact red background, got {background:?}");
    // The green square sits at row 30 from the top: +Y runs down the screen.
    let green = pixel(&pixels, 50, 30);
    assert!(green[1].abs_diff(188) <= 2 && green[2] < 5, "opaque green hides the deeper blue square, got {green:?}");
    let below = pixel(&pixels, 50, 70);
    assert!(below[0] > 150 && below[1] < 5, "the lower half shows the background there, got {below:?}");
    // Half white over 0.5 red: red ≈ 0.75, green and blue ≈ 0.5 (straight alpha).
    let blended = pixel(&pixels, 150, 50);
    assert!(blended[0] > blended[1] && blended[1] > 100 && blended[2] > 100, "blended white over red, got {blended:?}");
}
