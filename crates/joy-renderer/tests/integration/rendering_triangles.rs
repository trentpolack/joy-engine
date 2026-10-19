// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The arcade triangle renderer with a game-style shader: a custom uniform block, the top-down camera, opaque depth,
//! and blended transparency. Skips without a GPU adapter; native only (browsers cannot block on readback).
#![cfg(not(target_arch = "wasm32"))]

use joy_core::LinearColor;
use joy_core::math::Vec2;
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
    add_circle,
    add_quad,
    sort_top_down_transparent_triangles,
    top_down_orthographic,
};

/// A shader in the shape SPACE SCUFFLES and BLAST GARDEN author: an 80-byte block (camera matrix plus frame values) and
/// the TypeScript post-processing markers.
const SHADER: &str = r#"
struct Uniforms {
  viewProjection: mat4x4f,
  frame: vec4f,
  // JOY_POST_UNIFORMS_MACRO
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct VertexOutput {
  @builtin(position) position: vec4f,
  @location(0) color: vec4f,
};

// JOY_POST_DECLARATIONS_MACRO

@vertex
fn vertexMain(@location(0) position: vec3f, @location(1) color: vec4f) -> VertexOutput {
  var output: VertexOutput;
  output.position = uniforms.viewProjection * vec4f(position, 1.0);
  output.color = color;
  return output;
}

@fragment
fn fragmentMain(input: VertexOutput) -> @location(0) vec4f {
  let flashed = input.color.rgb + input.color.rgb * uniforms.frame.y * 0.18;
  let sourceColor = vec4f(flashed, input.color.a);
  // JOY_POST_APPLY_MACRO
}
"#;

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
fn custom_shader_draws_opaque_depth_then_blended_transparency() {
    let Some(gpu) = gpu() else {
        return;
    };
    let target = OffscreenTarget::new(&gpu, WIDTH, HEIGHT).unwrap();
    let options = TriangleRendererOptions { shader: SHADER.to_owned(), uniform_size: 80, post_process: None };
    let mut renderer = TriangleRenderer::new(&gpu, CAPTURE_FORMAT, options).unwrap();
    renderer.resize(WIDTH, HEIGHT, 1.0);

    // Logical world: 200×100 units centered on (100, 50); the camera sits at z = 100.
    let camera = TopDownCamera { height: 100.0, near: 1.0, far: 201.0 };
    let matrix = top_down_orthographic(WIDTH as f32, HEIGHT as f32, Vec2::new(100.0, 50.0), camera).unwrap();
    let mut uniforms = matrix.to_cols_array().to_vec();
    uniforms.extend([0.0, 0.0, 0.0, 0.0]); // frame: seconds, flash, padding

    // Opaque: a red background circle far away and a green square nearer the camera on the left.
    let mut opaque = Vec::new();
    add_circle(&mut opaque, 100.0, 50.0, 400.0, LinearColor::new(0.5, 0.0, 0.0, 1.0), 24, -50.0);
    add_quad(&mut opaque, 50.0, 50.0, 30.0, LinearColor::new(0.0, 0.5, 0.0, 1.0), std::f32::consts::FRAC_PI_4, 0.0);
    // Transparent: a half-white square on the right, drawn above the background.
    let mut transparent: Vec<ColorVertex> = Vec::new();
    add_quad(
        &mut transparent,
        150.0,
        50.0,
        30.0,
        LinearColor::new(1.0, 1.0, 1.0, 0.5),
        std::f32::consts::FRAC_PI_4,
        10.0,
    );
    // A transparent square *below* the green opaque square must be hidden by depth.
    add_quad(
        &mut transparent,
        50.0,
        50.0,
        10.0,
        LinearColor::new(0.0, 0.0, 1.0, 1.0),
        std::f32::consts::FRAC_PI_4,
        -10.0,
    );
    let transparent = sort_top_down_transparent_triangles(&transparent);

    let frame = TriangleFrame {
        opaque: &opaque,
        transparent: &transparent,
        uniforms: bytemuck::cast_slice(&uniforms),
        clear_color: LinearColor::new(0.0, 0.0, 0.0, 1.0),
        time_seconds: 0.0,
    };
    renderer.render(target.view(), &frame).unwrap();
    let pixels = target.read_rgba(&gpu).unwrap();

    let background = pixel(&pixels, 100, 10);
    assert!(background[0] > 100 && background[1] < 20, "red background, got {background:?}");
    let green = pixel(&pixels, 50, 50);
    assert!(green[1] > 100 && green[2] < 20, "opaque green hides the deeper blue square, got {green:?}");
    let blended = pixel(&pixels, 150, 50);
    // Half white over 0.5 red: red ≈ 0.75, green and blue ≈ 0.5 (straight alpha).
    assert!(blended[0] > blended[1] && blended[1] > 100 && blended[2] > 100, "blended white over red, got {blended:?}");

    // Wrong uniform sizes are reported, not uploaded.
    let short = TriangleFrame { uniforms: &[0; 16], ..frame };
    assert!(renderer.render(target.view(), &short).is_err());
}
