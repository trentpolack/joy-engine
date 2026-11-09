// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scene-scaling checks: a large retained scene with shadowed point lights, the CPU time spent preparing and recording
//! its frames, and how much shadow work a frame repeats.
//!
//! `frame_preparation_benchmark` is ignored by default because its timings only mean something on a quiet machine; run
//! it with `cargo test -p joy-renderer --release -- --ignored --nocapture frame_preparation_benchmark`. Set
//! `JOY_RENDER_CAPTURE_DIR` to keep a PNG of the benchmark scene. Native only, like the other headless checks.
#![cfg(not(target_arch = "wasm32"))]

use std::time::Instant;

use joy_core::LinearColor;
use joy_core::Transform;
use joy_core::math::Vec3;
use joy_renderer::{
    BackendPreference,
    DirectionalLight,
    FrameInputs,
    FramePhase,
    GpuContext,
    MeshAsset,
    MeshInstance,
    MeshInstanceId,
    OffscreenTarget,
    OrbitCamera,
    PointLight,
    RenderScene,
    Renderer,
    RendererOptions,
};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;

/// Crates per side of the benchmark grid; the scene holds this many squared, plus the ground.
const GRID_SIDE: usize = 64;
/// World units between neighboring crates.
const GRID_SPACING: f32 = 3.0;
/// Shadow-casting point lights spread over the grid.
const BENCHMARK_LIGHTS: usize = 8;
/// Reach of each benchmark light in world units.
const BENCHMARK_LIGHT_RANGE: f32 = 14.0;
/// Crates moved every frame in the moving scenario.
const MOVING_CRATES: usize = 64;
/// Frames rendered before timing starts, so pipelines compile and buffers settle.
const WARM_UP_FRAMES: usize = 3;
/// Frames timed per scenario.
const TIMED_FRAMES: usize = 30;

/// Create a headless device, or `None` (with a note) when the machine has no adapter.
fn gpu() -> Option<GpuContext> {
    match pollster::block_on(GpuContext::headless(BackendPreference::Auto)) {
        Ok(gpu) => Some(gpu),
        Err(error) => {
            eprintln!("skipping GPU test: {error}");
            None
        }
    }
}

/// Unit cube centered on the origin with per-face vertices.
fn unit_cube(color: [f32; 3]) -> MeshAsset {
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
                positions.push((normal[axis] + (u[axis] * du) + (v[axis] * dv)) * 0.5);
            }
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let colors: Vec<f32> = (0..positions.len() / 3).flat_map(|_| color).collect();
    MeshAsset::new(&crate::flat_mesh(&positions, &indices, &colors)).expect("cube geometry is valid")
}

/// A key light from high in the south-west, casting cascades.
fn key_light() -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::new(0.35, 0.85, 0.4).normalize(),
        color: LinearColor::rgb(1.0, 0.96, 0.86),
        intensity: 2.0,
        angular_radius: 0.00465,
        casts_shadow: true,
    }
}

/// The world position of grid crate `index`, resting on the ground.
fn crate_position(index: usize) -> Vec3 {
    let half = (GRID_SIDE as f32 - 1.0) * 0.5 * GRID_SPACING;
    let column = (index % GRID_SIDE) as f32;
    let row = (index / GRID_SIDE) as f32;
    Vec3::new((column * GRID_SPACING) - half, 0.5, (row * GRID_SPACING) - half)
}

/// A ground slab and a grid of crates drawn from a few shared assets, returning the crate handles in grid order.
fn crate_field() -> (RenderScene, Vec<MeshInstanceId>) {
    let mut scene = RenderScene::new();
    let extent = GRID_SIDE as f32 * GRID_SPACING;
    scene.insert(MeshInstance::new(unit_cube([0.34, 0.38, 0.36])).with_transform(Transform::new(
        Vec3::new(0.0, -0.25, 0.0),
        Vec3::ZERO,
        Vec3::new(extent, 0.5, extent),
    )));
    let palette = [unit_cube([0.95, 0.36, 0.12]), unit_cube([0.2, 0.55, 0.85]), unit_cube([0.85, 0.8, 0.3])];
    let mut crates = Vec::with_capacity(GRID_SIDE * GRID_SIDE);
    for index in 0..(GRID_SIDE * GRID_SIDE) {
        let asset = palette[index % palette.len()].clone();
        let transform = Transform::new(crate_position(index), Vec3::new(0.0, index as f32 * 0.37, 0.0), Vec3::ONE);
        crates.push(scene.insert(MeshInstance::new(asset).with_transform(transform)));
    }
    (scene, crates)
}

/// Shadow-casting point lights in a ring over the middle of the grid.
fn benchmark_lights() -> Vec<PointLight> {
    (0..BENCHMARK_LIGHTS)
        .map(|index| {
            let angle = (index as f32 / BENCHMARK_LIGHTS as f32) * std::f32::consts::TAU;
            PointLight {
                position: Vec3::new(angle.cos() * 18.0, 3.0, angle.sin() * 18.0),
                color: LinearColor::rgb(6.0, 4.5, 3.0),
                range: BENCHMARK_LIGHT_RANGE,
                casts_shadow: true,
            }
        })
        .collect()
}

/// The benchmark camera, looking down over the lit middle of the grid.
fn benchmark_camera(yaw: f32) -> OrbitCamera {
    OrbitCamera {
        target: Vec3::ZERO,
        distance: 60.0,
        radius: 40.0,
        yaw,
        pitch: 0.6,
        ..OrbitCamera::default()
    }
}

/// One timed frame: the CPU milliseconds `Renderer::encode` took, and the renderer profiler's milliseconds for the
/// preparation phase (everything before the first pass records) of the latest profiled frame.
#[derive(Clone, Copy, Debug)]
struct FrameTiming {
    encode_milliseconds: f64,
    prepare_milliseconds: f64,
}

/// Record one frame into a fresh encoder and time `Renderer::encode`; the frame is then submitted and waited on outside
/// the timing, so GPU work never overlaps the next measurement.
fn timed_frame(
    gpu: &GpuContext,
    renderer: &mut Renderer,
    scene: &RenderScene,
    camera: &OrbitCamera,
    inputs: &FrameInputs<'_>,
    target: &OffscreenTarget,
) -> FrameTiming {
    let mut encoder = gpu.device().create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("benchmark frame"),
    });
    let start = Instant::now();
    renderer.encode(&mut encoder, scene, camera, inputs, target.view()).expect("frame encodes");
    let encode_milliseconds = start.elapsed().as_secs_f64() * 1000.0;
    gpu.queue().submit([encoder.finish()]);
    let _ = gpu.device().poll(wgpu::PollType::wait_indefinitely());
    let prepare = renderer.frame_profile().and_then(|profile| {
        profile.phases.iter().find(|timing| timing.phase == FramePhase::Prepare).map(|timing| timing.cpu_milliseconds)
    });
    FrameTiming {
        encode_milliseconds,
        prepare_milliseconds: prepare.unwrap_or(0.0),
    }
}

/// Mean and slowest encode milliseconds, and mean preparation milliseconds, of a run of frames.
fn summarize(timings: &[FrameTiming]) -> (f64, f64, f64) {
    let count = timings.len() as f64;
    let mean = timings.iter().map(|timing| timing.encode_milliseconds).sum::<f64>() / count;
    let slowest = timings.iter().map(|timing| timing.encode_milliseconds).fold(0.0_f64, f64::max);
    let prepare = timings.iter().map(|timing| timing.prepare_milliseconds).sum::<f64>() / count;
    (mean, slowest, prepare)
}

/// Time CPU frame preparation and recording for a few thousand instances under a shadowing key light and eight
/// shadowed point lights, in three scenarios: a still scene and camera, a still camera while some crates move every
/// frame, and a still scene under an orbiting camera.
#[test]
#[ignore = "timing benchmark; run with --ignored --nocapture"]
fn frame_preparation_benchmark() {
    let Some(gpu) = gpu() else {
        return;
    };
    let (mut scene, crates) = crate_field();
    let lights = benchmark_lights();
    let inputs = FrameInputs {
        directional_light: Some(key_light()),
        lights: &lights,
        ambient: Some(0.25),
        clear_color: LinearColor::rgb(0.05, 0.06, 0.08),
        ..FrameInputs::default()
    };
    let target = OffscreenTarget::new(&gpu, WIDTH, HEIGHT).expect("capture target");
    let mut renderer = Renderer::new(&gpu, joy_renderer::CAPTURE_FORMAT, RendererOptions::default());
    renderer.resize(WIDTH, HEIGHT, 1.0);
    renderer.set_profiling(true);
    let still_camera = benchmark_camera(0.75);
    for _ in 0..WARM_UP_FRAMES {
        timed_frame(&gpu, &mut renderer, &scene, &still_camera, &inputs, &target);
    }

    // Still scene, still camera.
    let still: Vec<FrameTiming> =
        (0..TIMED_FRAMES).map(|_| timed_frame(&gpu, &mut renderer, &scene, &still_camera, &inputs, &target)).collect();
    let diagnostics = renderer.diagnostics().clone();

    // Still camera while a band of crates near the lights bobs every frame.
    let moving: Vec<FrameTiming> = (0..TIMED_FRAMES)
        .map(|frame| {
            for (offset, id) in crates.iter().skip((GRID_SIDE * GRID_SIDE) / 2).take(MOVING_CRATES).enumerate() {
                let instance = scene.get_mut(*id).expect("benchmark crates stay live");
                let lift = ((frame + offset) as f32 * 0.4).sin().abs();
                instance.transform.position.y = 0.5 + lift;
            }
            timed_frame(&gpu, &mut renderer, &scene, &still_camera, &inputs, &target)
        })
        .collect();

    // Still scene under an orbiting camera.
    let orbit: Vec<FrameTiming> = (0..TIMED_FRAMES)
        .map(|frame| {
            let camera = benchmark_camera(0.75 + (frame as f32 * 0.01));
            timed_frame(&gpu, &mut renderer, &scene, &camera, &inputs, &target)
        })
        .collect();

    let instances = scene.len();
    println!(
        "frame preparation over {instances} instances, {BENCHMARK_LIGHTS} shadowed point lights ({} visible, {} \
         culled, {} shadow views, {} caster draws)",
        diagnostics.visible, diagnostics.culled, diagnostics.shadow_views, diagnostics.shadow_casters
    );
    for (label, timings) in [("still", &still), ("moving", &moving), ("orbit", &orbit)] {
        let (mean, slowest, prepare) = summarize(timings);
        println!(
            "  {label:>6}: encode mean {mean:.3} ms (slowest {slowest:.3} ms), preparation {prepare:.3} ms, over \
             {TIMED_FRAMES} frames"
        );
    }

    if let Ok(directory) = std::env::var("JOY_RENDER_CAPTURE_DIR") {
        let pixels = render_capture(&gpu, &mut renderer, &scene, &still_camera, &inputs, &target);
        std::fs::create_dir_all(&directory).expect("capture directory");
        let path = std::path::Path::new(&directory).join("scaling-crate-field.png");
        image::save_buffer(&path, &pixels, WIDTH, HEIGHT, image::ColorType::Rgba8).expect("png written");
    }
}

/// Render one frame into `target` and read it back.
fn render_capture(
    gpu: &GpuContext,
    renderer: &mut Renderer,
    scene: &RenderScene,
    camera: &OrbitCamera,
    inputs: &FrameInputs<'_>,
    target: &OffscreenTarget,
) -> Vec<u8> {
    renderer.render(scene, camera, inputs, target.view()).expect("frame renders");
    target.read_rgba(gpu).expect("readback")
}

/// A ground slab with a crate beside each of three point lights spread far apart, so each light reaches only its own
/// crate (and the ground).
fn lantern_row() -> (RenderScene, Vec<MeshInstanceId>, Vec<PointLight>) {
    let mut scene = RenderScene::new();
    scene.insert(MeshInstance::new(unit_cube([0.34, 0.38, 0.36])).with_transform(Transform::new(
        Vec3::new(0.0, -0.25, 0.0),
        Vec3::ZERO,
        Vec3::new(90.0, 0.5, 30.0),
    )));
    let mut crates = Vec::new();
    let mut lights = Vec::new();
    for x in [-30.0, 0.0, 30.0] {
        let position = Vec3::new(x + 1.5, 0.5, 0.0);
        crates.push(scene.insert(MeshInstance::new(unit_cube([0.95, 0.36, 0.12])).with_transform(Transform::new(
            position,
            Vec3::ZERO,
            Vec3::ONE,
        ))));
        lights.push(PointLight {
            position: Vec3::new(x, 2.5, 0.0),
            color: LinearColor::rgb(8.0, 6.0, 4.0),
            range: 8.0,
            casts_shadow: true,
        });
    }
    (scene, crates, lights)
}

/// Point-light faces kept from earlier frames render again only when a caster inside that light's reach changes, and
/// a frame drawn with cached maps matches a fresh renderer's.
#[test]
fn cached_shadow_maps_render_again_only_where_casters_change() {
    let Some(gpu) = gpu() else {
        return;
    };
    let (mut scene, crates, lights) = lantern_row();
    let camera = OrbitCamera {
        distance: 70.0,
        radius: 40.0,
        yaw: 0.0,
        pitch: 0.7,
        ..OrbitCamera::default()
    };
    let inputs = FrameInputs {
        lights: &lights,
        ambient: Some(0.2),
        ..FrameInputs::default()
    };
    let target = OffscreenTarget::new(&gpu, WIDTH, HEIGHT).expect("capture target");
    let mut renderer = Renderer::new(&gpu, joy_renderer::CAPTURE_FORMAT, RendererOptions::default());
    renderer.resize(WIDTH, HEIGHT, 1.0);
    let rendered_after = |renderer: &mut Renderer, scene: &RenderScene| {
        renderer.render(scene, &camera, &inputs, target.view()).expect("frame renders");
        let diagnostics = renderer.diagnostics();
        assert_eq!(diagnostics.shadow_point_lights, 3);
        assert_eq!(diagnostics.shadow_views, 18, "every light's faces stay in use, rendered or cached");
        diagnostics.shadow_maps_rendered
    };

    // The first frame renders every face; a still scene renders none after it, and its kept maps still report the
    // casters they hold.
    assert_eq!(rendered_after(&mut renderer, &scene), 18);
    let held_casters = renderer.diagnostics().shadow_casters;
    assert!(held_casters > 0);
    assert_eq!(rendered_after(&mut renderer, &scene), 0);
    assert_eq!(renderer.diagnostics().shadow_casters, held_casters);

    // Moving the middle crate renders only the middle light's six faces, once.
    scene.get_mut(crates[1]).unwrap().transform.position.z = 1.0;
    assert_eq!(rendered_after(&mut renderer, &scene), 6);
    assert_eq!(rendered_after(&mut renderer, &scene), 0);

    // Edits shadows do not read (a tint on an opaque crate) render nothing; adding and removing a caster beside the
    // last light renders only that light.
    scene.get_mut(crates[0]).unwrap().tint = LinearColor::rgb(0.2, 0.9, 0.3);
    assert_eq!(rendered_after(&mut renderer, &scene), 0);
    let visitor = scene.insert(MeshInstance::new(unit_cube([0.2, 0.55, 0.85])).with_transform(Transform::new(
        Vec3::new(28.0, 0.5, 1.5),
        Vec3::ZERO,
        Vec3::ONE,
    )));
    assert_eq!(rendered_after(&mut renderer, &scene), 6);
    assert!(scene.remove(visitor));
    assert_eq!(rendered_after(&mut renderer, &scene), 6);

    // The cached frame draws what a renderer without a cache draws.
    let cached = target.read_rgba(&gpu).expect("readback");
    let mut fresh = Renderer::new(&gpu, joy_renderer::CAPTURE_FORMAT, RendererOptions::default());
    fresh.resize(WIDTH, HEIGHT, 1.0);
    fresh.render(&scene, &camera, &inputs, target.view()).expect("frame renders");
    assert_eq!(fresh.diagnostics().shadow_maps_rendered, 18);
    assert!(cached == target.read_rgba(&gpu).expect("readback"), "cached shadow maps match freshly rendered ones");
    if let Ok(directory) = std::env::var("JOY_RENDER_CAPTURE_DIR") {
        std::fs::create_dir_all(&directory).expect("capture directory");
        let path = std::path::Path::new(&directory).join("scaling-cached-lanterns.png");
        image::save_buffer(&path, &cached, WIDTH, HEIGHT, image::ColorType::Rgba8).expect("png written");
    }
}
