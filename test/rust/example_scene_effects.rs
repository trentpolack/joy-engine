// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The SCENE EFFECTS example (`examples/scene-effects`) on a real device, ported from its browser captures: the room
//! renders with 27 shadowed point lights, effect presets and temporal tests change the frame without blanking it, the
//! shadow fixture's off-camera blocker shadows the floor through five lights, and the shadow-material fixture's masked,
//! PBR, and vertex-color casters and receivers shadow one another.
//!
//! Needs a GPU adapter (llvmpipe or SwiftShader is enough); without one each test prints a note and returns. Set
//! `JOY_RENDER_CAPTURE_DIR` to keep PNG captures. Native only.
#![cfg(not(target_arch = "wasm32"))]

#[path = "../../examples/scene-effects/src/mod.rs"]
mod example;

use std::collections::HashMap;

use example::config;
use example::scene_effects_example::SceneEffectsExample;
use glam::{Vec2, Vec3};
use joy_engine::core::{MaterialAlphaMode, PbrMaterial, PbrMaterialTextures, PbrMesh};
use joy_engine::development::EffectPreset;
use joy_engine::rendering::{
    BackendPreference, CAPTURE_FORMAT, Camera, CelestialLight, EnvironmentLighting, FrameInputs, GpuContext, MeshAsset,
    MeshChannels, MeshInstance, OffscreenTarget, OrbitCamera, RenderScene, Renderer, RendererOptions, Shading,
    TextureAsset, project_to_viewport,
};
use joy_engine::{HeadlessOptions, HeadlessShell};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 400;

/// A headless shell with the lab's renderer settings, or `None` (with a note) without an adapter.
fn shell() -> Option<HeadlessShell> {
    let options =
        HeadlessOptions { width: WIDTH, height: HEIGHT, renderer: config::renderer_options(), ..Default::default() };
    match HeadlessShell::new(options) {
        Ok(shell) => Some(shell),
        Err(error) => {
            eprintln!("skipping GPU test: {error}");
            None
        }
    }
}

/// Run `frames` frames and return the last one, saving it as `name` when captures are requested.
fn frames(shell: &mut HeadlessShell, lab: &mut SceneEffectsExample, frames: u32, name: &str) -> Vec<u8> {
    shell.run_frames(lab, frames).expect("lab frames render");
    if let Ok(directory) = std::env::var("JOY_RENDER_CAPTURE_DIR") {
        std::fs::create_dir_all(&directory).expect("capture directory");
        shell.save_png(&std::path::Path::new(&directory).join(format!("{name}.png"))).expect("png written");
    }
    shell.capture().expect("readback")
}

/// Mean absolute per-channel difference between two captures.
fn difference(left: &[u8], right: &[u8]) -> f32 {
    let total: u64 = left.iter().zip(right).map(|(a, b)| u64::from(a.abs_diff(*b))).sum();
    total as f32 / left.len() as f32
}

fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
    let offset = ((y * width + x) * 4) as usize;
    [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
}

#[test]
fn the_room_renders_with_shadowed_point_lights_and_effect_presets() {
    let Some(mut shell) = shell() else {
        return;
    };
    let mut lab = SceneEffectsExample::new();
    shell.start(&mut lab).expect("lab starts");
    let baseline = frames(&mut shell, &mut lab, 3, "example-scene-effects-baseline");
    let snapshot = lab.snapshot();
    assert_eq!((snapshot.lights, snapshot.casting_lights, snapshot.meshes), (27, 27, 25));
    assert_eq!(snapshot.diagnostics.shadow_point_lights, 27, "every room light casts");
    assert_eq!(snapshot.diagnostics.shadow_views, 27 * 6, "six faces per light and no sun");
    assert!(snapshot.paused && snapshot.time_seconds == 2.0, "the clock starts frozen at two seconds");
    let lit = baseline
        .chunks_exact(4)
        .filter(|pixel| u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2]) > 60)
        .count();
    assert!(lit > (WIDTH * HEIGHT / 3) as usize, "the room fills the view ({lit} lit pixels)");

    // The red wall is left of the green wall (the image centers beside the sidebar).
    let left_wall = pixel(&baseline, WIDTH, 30, 200);
    let right_wall = pixel(&baseline, WIDTH, 360, 230);
    assert!(left_wall[0] > left_wall[1] * 2, "left wall is red: {left_wall:?}");
    assert!(right_wall[1] > right_wall[0] * 2, "right wall is green: {right_wall:?}");

    // Combined turns on the lighting set and bloom; on this backend the effects visibly change the frame.
    lab.controls_mut().select_preset(EffectPreset::Combined);
    let combined = frames(&mut shell, &mut lab, 4, "example-scene-effects-combined");
    let effects = lab.snapshot().effects;
    assert!(effects.gtao.is_some() && effects.ssgi.is_some() && effects.taa.is_some() && effects.ssr.is_some());
    assert_eq!(effects.gtao.unwrap().radius, 2.2, "the lab's tuning, not the engine defaults");
    assert!(shell.renderer().post_processing().bloom_strength == config::BLOOM_STRENGTH);
    if shell.renderer().scene_effects_capabilities().available {
        assert!(difference(&baseline, &combined) > 0.5, "effects change the frame");
    }

    // Temporal tests: motion with a stepped clock, a camera cut, then all off again.
    lab.set_motion(true, true);
    lab.step();
    let moving = frames(&mut shell, &mut lab, 3, "example-scene-effects-motion");
    assert!((lab.snapshot().time_seconds - (2.0 + config::STEP_SECONDS)).abs() < 1e-9);
    assert!(difference(&combined, &moving) > 0.1, "motion moves the block and camera");
    lab.camera_cut();
    lab.controls_mut().select_preset(EffectPreset::Off);
    frames(&mut shell, &mut lab, 2, "example-scene-effects-cut");
    assert!(!lab.snapshot().effects.any_enabled());
    assert_eq!(shell.renderer().post_processing().bloom_strength, 0.0);
}

#[test]
fn replacing_the_scene_swaps_the_wall_colors() {
    let Some(mut shell) = shell() else {
        return;
    };
    let mut lab = SceneEffectsExample::new();
    shell.start(&mut lab).expect("lab starts");
    frames(&mut shell, &mut lab, 1, "example-scene-effects-room");
    lab.replace_scene();
    let alternate = frames(&mut shell, &mut lab, 1, "example-scene-effects-alternate-room");
    let left_wall = pixel(&alternate, WIDTH, 30, 200);
    assert!(lab.snapshot().alternate_room);
    assert!(left_wall[2] > left_wall[0] * 2, "the replacement's left wall is blue: {left_wall:?}");
}

/// The TypeScript `shadows-browser` capture: an off-camera blocker shadows the floor from five lights, one light can
/// opt out of casting while still lighting, and the fifth light illuminates on its own.
#[test]
fn the_shadow_fixture_casts_from_an_off_camera_blocker() {
    let Some(mut shell) = shell() else {
        return;
    };
    let mut lab = SceneEffectsExample::new();
    shell.start(&mut lab).expect("lab starts");
    lab.shadow_fixture(true);
    let shadowed = frames(&mut shell, &mut lab, 2, "example-scene-effects-off-camera-caster");
    let snapshot = lab.snapshot();
    assert_eq!(snapshot.diagnostics.shadow_point_lights, 5);
    assert_eq!(snapshot.diagnostics.shadow_views, 30);
    assert!(snapshot.diagnostics.culled >= 1, "the high blocker is outside the camera frustum");
    assert!(snapshot.diagnostics.shadow_casters > snapshot.diagnostics.visible, "culled geometry still casts");

    lab.set_light_casting(0, false).unwrap();
    let unshadowed = frames(&mut shell, &mut lab, 2, "example-scene-effects-primary-light-casting-disabled");
    assert_eq!(lab.snapshot().diagnostics.shadow_point_lights, 4);
    assert_eq!(lab.snapshot().lights, 5, "opting out keeps the light");
    assert!(difference(&shadowed, &unshadowed) > 0.2, "casting opt-out changes visible coverage");

    for index in 0..5 {
        lab.set_light_color(index, [0.0; 3]).unwrap();
    }
    let no_direct = frames(&mut shell, &mut lab, 1, "example-scene-effects-no-direct-light");
    lab.set_light_color(4, [8.0; 3]).unwrap();
    let fifth_only = frames(&mut shell, &mut lab, 1, "example-scene-effects-fifth-light-only");
    assert!(difference(&no_direct, &fifth_only) > 0.5, "the fifth light illuminates visible surfaces");
    assert!(lab.set_light_casting(9, true).is_err() && lab.set_light_color(0, [f32::NAN; 3]).is_err());
}

// ---------------------------------------------------------------------------------------------------------------------
// Shadow-material fixture
// ---------------------------------------------------------------------------------------------------------------------

/// Capture size of the TypeScript fixture.
const FIXTURE_SIZE: u32 = 640;

/// Floor positions (z = 0.5) the TypeScript fixture samples: inside alternating checker cells of the blocker's
/// texture, plus one point beyond the blocker.
const SAMPLE_X: [f32; 5] = [-1.5, -0.5, 0.5, 1.5, 4.0];

/// A 32-pixel texture of 4×4 cells: white where `(x + y)` is odd, transparent elsewhere.
fn checker() -> TextureAsset {
    let mut pixels = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32 {
        for x in 0..32 {
            let filled = ((x / 8) + (y / 8)) % 2 == 1;
            pixels.extend_from_slice(if filled { &[255, 255, 255, 255] } else { &[0, 0, 0, 0] });
        }
    }
    TextureAsset::from_rgba8(32, 32, pixels).expect("checker texture")
}

fn fixture_material(alpha_mode: MaterialAlphaMode, base_color: Option<&str>) -> PbrMaterial {
    PbrMaterial {
        base_color: vec![0.6, 0.6, 0.6, 1.0],
        emissive: vec![0.0; 3],
        metallic: 0.0,
        roughness: 1.0,
        alpha_mode,
        alpha_cutoff: 0.5,
        textures: PbrMaterialTextures { base_color: base_color.map(str::to_owned), ..Default::default() },
    }
}

/// The fixture's PBR mesh: a 12-unit floor (unless the floor is vertex-colored instead) and a 4-unit textured blocker
/// 20 units up, far above the camera's view.
fn fixture_mesh(masked: bool, pbr_floor: bool) -> PbrMesh {
    let blocker = if masked { MaterialAlphaMode::Mask } else { MaterialAlphaMode::Opaque };
    PbrMesh {
        positions: vec![
            -6.0, 0.0, -6.0, 6.0, 0.0, -6.0, 6.0, 0.0, 6.0, -6.0, 0.0, 6.0, -2.0, 20.0, -2.0, 2.0, 20.0, -2.0, 2.0,
            20.0, 2.0, -2.0, 20.0, 2.0,
        ],
        normals: [0.0, 1.0, 0.0].repeat(8),
        colors: vec![1.0; 24],
        alphas: vec![1.0; 8],
        uvs: vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0],
        triangles: if pbr_floor { vec![0, 2, 1, 0, 3, 2, 4, 6, 5, 4, 7, 6] } else { vec![4, 6, 5, 4, 7, 6] },
        triangle_materials: if pbr_floor { vec![0, 0, 1, 1] } else { vec![1, 1] },
        materials: vec![
            fixture_material(MaterialAlphaMode::Opaque, None),
            fixture_material(blocker, Some("checker.png")),
        ],
    }
}

/// A horizontal vertex-color quad (the TypeScript colored triangle queue), lit like `Shading::Lit` meshes.
fn colored_quad(half: f32, height: f32, color: f32) -> MeshAsset {
    MeshAsset::from_channels(MeshChannels {
        positions: &[-half, height, -half, half, height, -half, half, height, half, -half, height, half],
        indices: &[0, 2, 1, 0, 3, 2],
        colors: &[color; 12],
        ..Default::default()
    })
    .expect("quad geometry")
}

/// Render one fixture variant from the TypeScript camera with light straight down, returning the red channel at each
/// sample.
fn capture_mask_shadow(
    gpu: &GpuContext,
    masked: bool,
    colored_blocker: bool,
    colored_receiver: bool,
    name: &str,
) -> Vec<u8> {
    let textures = HashMap::from([("checker.png", checker())]);
    let asset =
        MeshAsset::from_pbr_mesh(&fixture_mesh(masked, !colored_receiver), |source| textures.get(source).cloned())
            .expect("fixture mesh");
    let mut scene = RenderScene::new();
    scene.insert(MeshInstance::new(asset).with_shading(Shading::Pbr));
    if colored_receiver {
        scene.insert(MeshInstance::new(colored_quad(6.0, 0.0, 0.6)));
    }
    if colored_blocker {
        scene.insert(MeshInstance::new(colored_quad(2.0, 10.0, 1.0)));
    }

    let camera = OrbitCamera {
        target: Vec3::ZERO,
        radius: 6.0,
        distance: 18.0,
        pitch: 0.9,
        yaw: 0.0,
        near_clip: Some(0.1),
        far_clip: Some(100.0),
        ..OrbitCamera::default()
    };
    let sun = CelestialLight {
        direction: Vec3::Y,
        color: [1.0; 3],
        intensity: 4.0,
        angular_radius: 0.00465,
        casts_shadow: true,
    };
    let environment = EnvironmentLighting {
        elapsed_seconds: 0.0,
        time_of_day: 0.5,
        sun,
        moon: CelestialLight { intensity: 0.0, ..sun },
        primary: sun,
        skylight_color: [0.0; 3],
        skylight_intensity: 0.0,
        wind_velocity: Vec3::ZERO,
        scattering: [1.0, 0.6, 1.0, 1.0],
        fog_and_clouds: [0.0; 4],
        cloud_shape_and_shafts: [1800.0, 700.0, 0.0, 0.94],
        shaft_exposure_and_occlusion: [0.0, 1.0, 7.0],
    };
    let options = RendererOptions { atmosphere: false, ..RendererOptions::default() };
    let mut renderer = Renderer::new(gpu, CAPTURE_FORMAT, options);
    let target = OffscreenTarget::new(gpu, FIXTURE_SIZE, FIXTURE_SIZE).expect("capture target");
    renderer.resize(FIXTURE_SIZE, FIXTURE_SIZE, 1.0);
    let inputs = FrameInputs { environment: Some(&environment), ambient: Some(0.02), ..FrameInputs::default() };
    renderer.render(&scene, &camera, &inputs, target.view()).expect("fixture renders");
    let pixels = target.read_rgba(gpu).expect("readback");
    if let Ok(directory) = std::env::var("JOY_RENDER_CAPTURE_DIR") {
        std::fs::create_dir_all(&directory).expect("capture directory");
        let path = std::path::Path::new(&directory).join(format!("{name}.png"));
        image::save_buffer(&path, &pixels, FIXTURE_SIZE, FIXTURE_SIZE, image::ColorType::Rgba8).expect("png written");
    }

    let frame = camera.camera_frame(1.0);
    let size = Vec2::splat(FIXTURE_SIZE as f32);
    SAMPLE_X
        .iter()
        .map(|x| {
            let point = project_to_viewport(Vec3::new(*x, 0.0, 0.5), frame.view_projection(), size).expect("on screen");
            pixels[((point.y.round() as u32 * FIXTURE_SIZE + point.x.round() as u32) * 4) as usize]
        })
        .collect()
}

/// The TypeScript `shadow-material-fixture` regression: only the floor is visible; a high cutout casts vertically.
#[test]
fn masked_pbr_casters_shadow_receivers_through_alpha_holes() {
    let Some(gpu) = pollster::block_on(GpuContext::headless(BackendPreference::Auto))
        .map_err(|error| {
            eprintln!("skipping GPU test: {error}");
        })
        .ok()
    else {
        return;
    };
    let solid = capture_mask_shadow(&gpu, false, false, false, "example-scene-effects-solid-caster");
    let masked = capture_mask_shadow(&gpu, true, false, false, "example-scene-effects-masked-caster");
    let colored = capture_mask_shadow(&gpu, true, true, false, "example-scene-effects-colored-caster");
    let colored_receiver = capture_mask_shadow(&gpu, false, false, true, "example-scene-effects-colored-receiver");
    let report = format!("solid {solid:?}, masked {masked:?}, colored {colored:?}, receiver {colored_receiver:?}");

    // Transparent cells light the floor; opaque cells keep their shadow.
    for index in [0, 2] {
        assert!(masked[index] > solid[index].saturating_add(60), "alpha holes must light receivers: {report}");
    }
    for index in [1, 3] {
        assert!(masked[index] <= solid[index].saturating_add(20), "opaque alpha cells retain shadow: {report}");
    }
    // A vertex-color caster shadows PBR receivers, and the PBR caster shadows vertex-color receivers while the floor
    // beyond it keeps its direct light.
    assert!(colored[..4].iter().all(|value| *value < 30), "colored casters shadow PBR receivers: {report}");
    assert!(colored_receiver[..4].iter().all(|value| *value < 30), "PBR casters shadow colored receivers: {report}");
    let darkest = *colored_receiver[..4].iter().max().unwrap();
    assert!(colored_receiver[4] > darkest.saturating_add(100), "normal bias preserves unoccluded light: {report}");
}
