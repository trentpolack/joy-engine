// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Headless rendering of particles through `FrameInputs::transparent_draws`: textured sprites sample their PNG (opaque
//! texels tinted by particle color, transparent texels showing the background), colored particles draw in the same
//! sorted stream, and opaque geometry in front hides particles behind it.
//!
//! Needs a GPU adapter (a software adapter such as llvmpipe is enough); without one the test prints a message and
//! returns. Set `JOY_RENDER_CAPTURE_DIR` to keep a PNG capture. Native only.
#![cfg(not(target_arch = "wasm32"))]

use glam::{Vec2, Vec3};
use joy_engine::core::Transform;
use joy_engine::particles::{
    ParticleCameraBasis, ParticleEffect, ParticleEffectOptions, ParticleTextureSet, compile_particle_effect,
    create_particle_effect_batches,
};
use joy_engine::rendering::{
    BackendPreference, Camera, FrameInputs, GpuContext, MeshAsset, MeshChannels, MeshInstance, OffscreenTarget,
    OrbitCamera, RenderScene, Renderer, RendererOptions, Shading, project_to_viewport,
};
use serde_json::json;

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;

/// The engine's default cross texture: opaque white cross, transparent corners.
const CROSS: &str = joy_engine::particles::DEFAULT_PARTICLE_TEXTURE;

/// A box of full extents `size` with flat white faces (two triangles per face).
fn block(size: f32) -> MeshAsset {
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ([-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]),
        ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        ([0.0, -1.0, 0.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ];
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    for (normal, u, v) in faces {
        let base = (positions.len() / 3) as u32;
        for (du, dv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            for axis in 0..3 {
                positions.push((normal[axis] + (u[axis] * du) + (v[axis] * dv)) * size * 0.5);
            }
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    MeshAsset::from_channels(MeshChannels { positions: &positions, indices: &indices, ..Default::default() })
        .expect("block geometry is valid")
}

fn pixel(pixels: &[u8], point: Vec2) -> [u8; 3] {
    let offset = (((point.y as u32) * WIDTH + (point.x as u32)) * 4) as usize;
    [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
}

#[test]
fn textured_and_colored_particles_blend_over_the_scene_with_depth_testing() {
    let gpu = match pollster::block_on(GpuContext::headless(BackendPreference::Auto)) {
        Ok(gpu) => gpu,
        Err(error) => {
            eprintln!("skipping GPU test: {error}");
            return;
        }
    };

    // Two green textured sprites at x = -2.5 and x = 2.5, and one red colored square above them.
    let definition = compile_particle_effect(&json!({
        "version": 1, "name": "Headless", "maxParticles": 8, "parameters": {},
        "emitters": [
            {"id": "sprites", "enabled": true, "root": true, "duration": 0, "rate": 0,
             "bursts": [{"time": 0, "count": 2}], "lifetime": 5, "renderer": "textured", "texture": {"source": CROSS},
             "spawn": "x = (index - 0.5) * 5; size = 1.5; color = rgba(0, 1, 0, 1);", "update": ""},
            {"id": "marker", "enabled": true, "root": true, "duration": 0, "rate": 0,
             "bursts": [{"time": 0, "count": 1}], "lifetime": 5, "renderer": "square",
             "spawn": "y = 2.5; size = 0.6; color = rgba(1, 0, 0, 1);", "update": ""}
        ]
    }))
    .unwrap();
    let effect = ParticleEffect::new(&definition, ParticleEffectOptions::default()).unwrap();
    let mut textures = ParticleTextureSet::new();
    textures.prepare(&definition).unwrap();
    textures.activate(Some(&definition));

    // Camera on +Z looking at the origin; an unlit white block sits between it and the right-hand sprite.
    let camera = OrbitCamera { distance: 10.0, yaw: 0.0, pitch: 0.0, ..OrbitCamera::default() };
    let basis = ParticleCameraBasis::from(camera.basis());
    let batches = create_particle_effect_batches(&effect, &basis, None, &[]).unwrap();
    let draws = textures.transparent_draws(&batches).unwrap();
    let mut scene = RenderScene::new();
    let mut occluder =
        MeshInstance::new(block(2.0)).with_transform(Transform::new(Vec3::new(2.5, 0.0, 2.0), Vec3::ZERO, Vec3::ONE));
    occluder.shading = Shading::Unlit;
    scene.insert(occluder);

    let inputs =
        FrameInputs { clear_color: [0.0, 0.0, 0.25, 1.0], transparent_draws: &draws, ..FrameInputs::default() };
    let target = OffscreenTarget::new(&gpu, WIDTH, HEIGHT).expect("capture target");
    let mut renderer = Renderer::new(&gpu, joy_engine::rendering::CAPTURE_FORMAT, RendererOptions::default());
    renderer.resize(WIDTH, HEIGHT, 1.0);
    renderer.render(&scene, &camera, &inputs, target.view()).expect("frame renders");
    let pixels = target.read_rgba(&gpu).expect("readback");
    if let Ok(directory) = std::env::var("JOY_RENDER_CAPTURE_DIR") {
        std::fs::create_dir_all(&directory).expect("capture directory");
        let path = std::path::Path::new(&directory).join("particles-textured.png");
        image::save_buffer(&path, &pixels, WIDTH, HEIGHT, image::ColorType::Rgba8).expect("png written");
    }
    assert!(renderer.diagnostics().draw_calls >= 3, "{:?}", renderer.diagnostics());

    let size = Vec2::new(WIDTH as f32, HEIGHT as f32);
    let view_projection = camera.camera_frame(size.x / size.y).view_projection();
    let project = |point: Vec3| project_to_viewport(point, view_projection, size).expect("in front of the camera");

    // The left sprite's opaque center shows the green tint; its transparent corner shows the background.
    let background = pixel(&pixels, Vec2::new(4.0, 4.0));
    let center = pixel(&pixels, project(Vec3::new(-2.5, 0.0, 0.0)));
    assert!(center[1] > 120 && center[1] > center[0] + 60 && center[1] > center[2] + 60, "sprite center {center:?}");
    let corner = pixel(&pixels, project(Vec3::new(-2.5 - 1.35, 1.35, 0.0)));
    let difference = corner.iter().zip(background).map(|(a, b)| (i32::from(*a) - i32::from(b)).abs()).max().unwrap();
    assert!(difference <= 3, "transparent texels keep the background: {corner:?} vs {background:?}");

    // The colored square draws in the same stream.
    let marker = pixel(&pixels, project(Vec3::new(0.0, 2.5, 0.0)));
    assert!(marker[0] > 120 && marker[0] > marker[1] + 60, "colored particle {marker:?}");

    // The block in front hides the right sprite's center (depth test against opaque geometry).
    let hidden = pixel(&pixels, project(Vec3::new(2.5, 0.0, 0.0)));
    let neutral = hidden.iter().max().unwrap() - hidden.iter().min().unwrap() < 12;
    assert!(neutral && hidden[0] > 60, "the neutral block, not the green sprite, shows: {hidden:?}");
    let edge = pixel(&pixels, project(Vec3::new(2.5 - 1.35, 0.0, 0.0)));
    assert!(edge[1] > edge[0] + 60, "the sprite's arm left of the block stays visible: {edge:?}");
}
