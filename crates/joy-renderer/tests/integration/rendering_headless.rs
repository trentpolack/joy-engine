// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Headless rendering checks. Each test renders a small scene offscreen and inspects pixels.
//!
//! These need a GPU adapter (a software adapter such as SwiftShader or llvmpipe is enough). Without one they print a
//! message and return instead of failing. Set `JOY_RENDER_CAPTURE_DIR` to also write PNG captures for review. Native
//! only: browsers cannot block on GPU readback.
#![cfg(not(target_arch = "wasm32"))]

use joy_core::LinearColor;
use joy_core::Transform;
use joy_core::math::Vec3;
use joy_renderer::{
    BackendPreference,
    DirectionalLight,
    EnvironmentLighting,
    FrameInputs,
    FramePhase,
    GpuContext,
    MAX_POINT_LIGHTS,
    MeshAsset,
    MeshInstance,
    OffscreenTarget,
    OrbitCamera,
    PointLight,
    RenderScene,
    Renderer,
    RendererOptions,
    SceneNode,
    Skylight,
};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;

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

/// A clear noon sky with the sun high in the south-west.
fn noon() -> EnvironmentLighting {
    let sun = DirectionalLight {
        direction: Vec3::new(0.35, 0.85, 0.4).normalize(),
        color: LinearColor::rgb(1.0, 0.96, 0.86),
        intensity: 5.0,
        angular_radius: 0.00465,
        casts_shadow: true,
    };
    let moon = DirectionalLight {
        direction: -sun.direction,
        color: LinearColor::rgb(0.24, 0.32, 0.55),
        intensity: 0.0,
        angular_radius: 0.00436,
        casts_shadow: true,
    };
    EnvironmentLighting {
        elapsed_seconds: 0.0,
        time_of_day: 0.5,
        sun,
        moon,
        primary: sun,
        skylight: Skylight::uniform(LinearColor::rgb(0.2, 0.42, 0.8), 0.45),
        wind_velocity: Vec3::new(4.0, 0.0, 0.0),
        scattering: [1.0, 0.6, 1.0, 1.0],
        fog_and_clouds: [0.002, 0.0, 0.0, 0.45],
        cloud_shape_and_shafts: [1800.0, 700.0, 0.65, 0.94],
        shaft_exposure_and_occlusion: [0.2, 1.0, 7.0],
    }
}

/// Render one frame and return RGBA8 pixels.
fn render(
    gpu: &GpuContext,
    scene: &RenderScene,
    camera: &OrbitCamera,
    inputs: &FrameInputs<'_>,
    name: &str,
) -> Vec<u8> {
    let target = OffscreenTarget::new(gpu, WIDTH, HEIGHT).expect("capture target");
    let mut renderer = Renderer::new(gpu, joy_renderer::CAPTURE_FORMAT, RendererOptions::default());
    renderer.resize(WIDTH, HEIGHT, 1.0);
    renderer.render(scene, camera, inputs, target.view()).expect("frame renders");
    let pixels = target.read_rgba(gpu).expect("readback");
    if let Ok(directory) = std::env::var("JOY_RENDER_CAPTURE_DIR") {
        std::fs::create_dir_all(&directory).expect("capture directory");
        let path = std::path::Path::new(&directory).join(format!("{name}.png"));
        image::save_buffer(&path, &pixels, WIDTH, HEIGHT, image::ColorType::Rgba8).expect("png written");
    }
    pixels
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let offset = ((y * WIDTH + x) * 4) as usize;
    [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
}

fn luminance(color: [u8; 3]) -> f32 {
    (0.2126 * f32::from(color[0])) + (0.7152 * f32::from(color[1])) + (0.0722 * f32::from(color[2]))
}

#[test]
fn sunlit_scene_renders_sky_ground_and_shadow() {
    let Some(gpu) = gpu() else {
        return;
    };

    let mut scene = RenderScene::new();
    let ground = unit_cube([0.31, 0.38, 0.39]);
    let crate_asset = unit_cube([0.95, 0.36, 0.12]);
    scene.insert(MeshInstance::new(ground).with_transform(Transform::new(
        Vec3::new(0.0, -0.25, 0.0),
        Vec3::ZERO,
        Vec3::new(30.0, 0.5, 30.0),
    )));
    scene.insert(MeshInstance::new(crate_asset).with_transform(Transform::new(
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::ZERO,
        Vec3::splat(2.0),
    )));

    // View from the side of the sun's direction so the shadow is not hidden behind the crate.
    let camera = OrbitCamera {
        distance: 14.0,
        pitch: 0.55,
        yaw: 0.72 + std::f32::consts::FRAC_PI_2,
        ..OrbitCamera::default()
    };
    let environment = noon();
    let inputs = FrameInputs {
        environment: Some(&environment),
        ..FrameInputs::default()
    };
    let pixels = render(&gpu, &scene, &camera, &inputs, "sunlit-scene");

    // The top rows show sky: bright and blue-dominant.
    let sky = pixel(&pixels, WIDTH / 2, 4);
    assert!(sky[2] > sky[0] && luminance(sky) > 60.0, "sky pixel {sky:?}");

    // Project the shadow region: the crate shadows ground on the side away from the sun.
    let renderer_size = joy_core::math::Vec2::new(WIDTH as f32, HEIGHT as f32);
    let frame = joy_renderer::Camera::camera_frame(&camera, renderer_size.x / renderer_size.y);
    let sun = environment.primary.direction;
    let shadowed_point = Vec3::new(-sun.x, 0.0, -sun.z).normalize() * 1.6;
    let lit_point = Vec3::new(sun.x, 0.0, sun.z).normalize() * 4.5;
    let shadowed = joy_renderer::project_to_viewport(shadowed_point, frame.view_projection(), renderer_size).unwrap();
    let lit = joy_renderer::project_to_viewport(lit_point, frame.view_projection(), renderer_size).unwrap();
    let shadowed_color = pixel(&pixels, shadowed.x as u32, shadowed.y as u32);
    let lit_color = pixel(&pixels, lit.x as u32, lit.y as u32);
    assert!(luminance(lit_color) > 40.0, "lit ground {lit_color:?}");
    assert!(
        luminance(shadowed_color) < luminance(lit_color) * 0.8,
        "shadowed ground {shadowed_color:?} should be darker than lit ground {lit_color:?}"
    );
}

/// Each shadow view draws only the casters inside its volume, so a caster far beyond the shadow distance costs no
/// shadow work.
#[test]
fn shadow_views_draw_only_the_casters_inside_them() {
    let Some(gpu) = gpu() else {
        return;
    };
    let mut scene = RenderScene::new();
    scene.insert(MeshInstance::new(unit_cube([0.31, 0.38, 0.39])).with_transform(Transform::new(
        Vec3::new(0.0, -0.25, 0.0),
        Vec3::ZERO,
        Vec3::new(30.0, 0.5, 30.0),
    )));
    scene.insert(MeshInstance::new(unit_cube([0.95, 0.36, 0.12])).with_transform(Transform::new(
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::ZERO,
        Vec3::splat(2.0),
    )));
    scene.insert(MeshInstance::new(unit_cube([0.95, 0.36, 0.12])).with_transform(Transform::new(
        Vec3::new(2000.0, 1.0, 0.0),
        Vec3::ZERO,
        Vec3::splat(2.0),
    )));
    let camera = OrbitCamera {
        distance: 14.0,
        pitch: 0.55,
        ..OrbitCamera::default()
    };
    let environment = noon();
    let inputs = FrameInputs {
        environment: Some(&environment),
        ..FrameInputs::default()
    };

    let target = OffscreenTarget::new(&gpu, WIDTH, HEIGHT).expect("capture target");
    let mut renderer = Renderer::new(&gpu, joy_renderer::CAPTURE_FORMAT, RendererOptions::default());
    renderer.resize(WIDTH, HEIGHT, 1.0);
    renderer.render(&scene, &camera, &inputs, target.view()).expect("frame renders");
    let diagnostics = renderer.diagnostics();
    assert!(diagnostics.shadows_active && diagnostics.shadow_views > 0);
    assert!(
        diagnostics.shadow_casters <= 2 * diagnostics.shadow_views,
        "the distant crate casts into no cascade: {} caster draws over {} views",
        diagnostics.shadow_casters,
        diagnostics.shadow_views
    );
}

/// A reduced render scale draws the scene at fewer pixels and upscales it into the unchanged output: the frame keeps
/// its size and view, screen-space queries do not move, and the image stays close to the full-resolution one.
#[test]
fn reduced_render_scale_upscales_into_the_full_output() {
    let Some(gpu) = gpu() else {
        return;
    };
    let mut scene = RenderScene::new();
    scene.insert(MeshInstance::new(unit_cube([0.31, 0.38, 0.39])).with_transform(Transform::new(
        Vec3::new(0.0, -0.25, 0.0),
        Vec3::ZERO,
        Vec3::new(30.0, 0.5, 30.0),
    )));
    scene.insert(MeshInstance::new(unit_cube([0.95, 0.36, 0.12])).with_transform(Transform::new(
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::ZERO,
        Vec3::splat(2.0),
    )));
    let camera = OrbitCamera {
        distance: 14.0,
        pitch: 0.55,
        ..OrbitCamera::default()
    };
    let environment = noon();
    let inputs = FrameInputs {
        environment: Some(&environment),
        ..FrameInputs::default()
    };
    let crate_center = Vec3::new(0.0, 1.0, 0.0);

    let target = OffscreenTarget::new(&gpu, WIDTH, HEIGHT).expect("capture target");
    let mut renderer = Renderer::new(&gpu, joy_renderer::CAPTURE_FORMAT, RendererOptions::default());
    renderer.resize(WIDTH, HEIGHT, 1.0);
    renderer.render(&scene, &camera, &inputs, target.view()).expect("full-scale frame renders");
    let full = target.read_rgba(&gpu).expect("readback");
    let full_projection = renderer.project(&camera, crate_center);
    assert_eq!(renderer.diagnostics().render_size, (WIDTH, HEIGHT));

    renderer.set_render_scale(0.5);
    renderer.render(&scene, &camera, &inputs, target.view()).expect("half-scale frame renders");
    let half = target.read_rgba(&gpu).expect("readback");
    assert_eq!(renderer.diagnostics().render_size, (WIDTH / 2, HEIGHT / 2));
    assert_eq!(renderer.physical_size(), (WIDTH, HEIGHT));
    assert_eq!(renderer.project(&camera, crate_center), full_projection);

    // Mean channel difference over the frame, in 8-bit steps: only edges and fine shading soften.
    let difference: f64 =
        full.iter().zip(&half).map(|(full, half)| f64::from(full.abs_diff(*half))).sum::<f64>() / full.len() as f64;
    assert!(difference < 1.5, "half-scale frame differs by {difference:.2} steps on average");
    let crate_pixel = full_projection.expect("the crate is in view");
    let (x, y) = (crate_pixel.x as u32, crate_pixel.y as u32);
    let (full_crate, half_crate) = (luminance(pixel(&full, x, y)), luminance(pixel(&half, x, y)));
    assert!((full_crate - half_crate).abs() < 4.0, "crate {full_crate} at full scale, {half_crate} at half");

    // Out-of-range scales clamp; non-finite ones restore full resolution.
    renderer.set_render_scale(0.05);
    assert_eq!(renderer.render_scale(), joy_renderer::MIN_RENDER_SCALE);
    renderer.set_render_scale(f32::NAN);
    assert_eq!(renderer.render_scale(), joy_renderer::MAX_RENDER_SCALE);
}

#[test]
fn point_lights_illuminate_without_an_environment() {
    let Some(gpu) = gpu() else {
        return;
    };

    let mut scene = RenderScene::new();
    scene.insert(MeshInstance::new(unit_cube([1.0, 1.0, 1.0])).with_transform(Transform::new(
        Vec3::ZERO,
        Vec3::ZERO,
        Vec3::new(10.0, 0.2, 10.0),
    )));
    let camera = OrbitCamera {
        distance: 8.0,
        pitch: 0.9,
        ..OrbitCamera::default()
    };

    let dark = FrameInputs {
        ambient: Some(0.0),
        ..FrameInputs::default()
    };
    let dark_pixels = render(&gpu, &scene, &camera, &dark, "point-light-off");
    let lights = [PointLight {
        position: Vec3::new(0.0, 1.5, 0.0),
        color: LinearColor::rgb(3.0, 2.0, 1.0),
        range: 6.0,
        casts_shadow: false,
    }];
    let lit = FrameInputs {
        ambient: Some(0.0),
        lights: &lights,
        ..FrameInputs::default()
    };
    let lit_pixels = render(&gpu, &scene, &camera, &lit, "point-light-on");

    let center = (WIDTH / 2, HEIGHT / 2);
    let off = pixel(&dark_pixels, center.0, center.1);
    let on = pixel(&lit_pixels, center.0, center.1);
    assert!(luminance(off) < 5.0, "unlit slab should be black: {off:?}");
    assert!(luminance(on) > 40.0 && on[0] > on[2], "warm light should brighten the slab: {on:?}");
}

/// A level may supply more point lights than a frame draws: the renderer keeps the strongest that reach the view and
/// reports the rest, instead of failing the frame.
#[test]
fn frames_with_more_point_lights_than_the_cap_draw_the_strongest() {
    let Some(gpu) = gpu() else {
        return;
    };
    let mut scene = RenderScene::new();
    scene.insert(MeshInstance::new(unit_cube([1.0, 1.0, 1.0])).with_transform(Transform::new(
        Vec3::ZERO,
        Vec3::ZERO,
        Vec3::new(10.0, 0.2, 10.0),
    )));
    let camera = OrbitCamera {
        distance: 8.0,
        pitch: 0.9,
        ..OrbitCamera::default()
    };

    // The warm light over the slab, forty dim lights in a ring around it, and one invalid light.
    let warm = PointLight {
        position: Vec3::new(0.0, 1.5, 0.0),
        color: LinearColor::rgb(3.0, 2.0, 1.0),
        range: 6.0,
        casts_shadow: false,
    };
    let dim = MAX_POINT_LIGHTS + 8;
    let mut lights: Vec<PointLight> = (0..dim)
        .map(|index| {
            let angle = index as f32 * std::f32::consts::TAU / dim as f32;
            PointLight {
                position: Vec3::new(angle.cos() * 2.5, 0.5, angle.sin() * 2.5),
                color: LinearColor::rgb(0.02, 0.02, 0.05),
                range: 1.0,
                casts_shadow: false,
            }
        })
        .collect();
    lights.push(warm);
    lights.push(PointLight {
        range: f32::NAN,
        ..warm
    });

    let target = OffscreenTarget::new(&gpu, WIDTH, HEIGHT).expect("capture target");
    let mut renderer = Renderer::new(&gpu, joy_renderer::CAPTURE_FORMAT, RendererOptions::default());
    renderer.resize(WIDTH, HEIGHT, 1.0);
    let inputs = FrameInputs {
        ambient: Some(0.0),
        lights: &lights,
        ..FrameInputs::default()
    };
    renderer.render(&scene, &camera, &inputs, target.view()).expect("extra lights never fail a frame");
    let diagnostics = renderer.diagnostics();
    assert_eq!(diagnostics.point_lights, MAX_POINT_LIGHTS);
    assert_eq!(diagnostics.point_lights_omitted, lights.len() - MAX_POINT_LIGHTS);

    // The warm light ranks first, so the slab lights as it does with that light alone.
    let pixels = target.read_rgba(&gpu).expect("readback");
    let center = pixel(&pixels, WIDTH / 2, HEIGHT / 2);
    assert!(luminance(center) > 40.0 && center[0] > center[2], "the strongest light is kept: {center:?}");
}

#[test]
fn point_lights_cast_soft_shadows_from_every_face() {
    let Some(gpu) = gpu() else {
        return;
    };
    let mut scene = RenderScene::new();
    scene.insert(MeshInstance::new(unit_cube([1.0, 1.0, 1.0])).with_transform(Transform::new(
        Vec3::new(0.0, -0.1, 0.0),
        Vec3::ZERO,
        Vec3::new(12.0, 0.2, 12.0),
    )));
    // A floating blocker between two lights: one beside it (a side face) and one above it (the downward face).
    scene.insert(MeshInstance::new(unit_cube([0.8, 0.8, 0.8])).with_transform(Transform::new(
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::ZERO,
        Vec3::splat(0.8),
    )));
    let camera = OrbitCamera {
        distance: 11.0,
        pitch: 1.1,
        ..OrbitCamera::default()
    };
    let frame = joy_renderer::Camera::camera_frame(&camera, WIDTH as f32 / HEIGHT as f32);
    let size = joy_core::math::Vec2::new(WIDTH as f32, HEIGHT as f32);

    for (name, light, shadowed_point) in [
        ("point-shadow-side", Vec3::new(-2.0, 2.0, 0.0), Vec3::new(2.0, 0.0, 0.0)),
        ("point-shadow-above", Vec3::new(0.0, 3.0, 2.0), Vec3::new(0.0, 0.0, -1.6)),
    ] {
        let mut colors = [[0_u8; 3]; 2];
        for (casts_shadow, color) in [false, true].into_iter().zip(&mut colors) {
            let lights = [PointLight {
                position: light,
                color: LinearColor::rgb(6.0, 6.0, 6.0),
                range: 12.0,
                casts_shadow,
            }];
            let inputs = FrameInputs {
                ambient: Some(0.0),
                lights: &lights,
                ..FrameInputs::default()
            };
            let pixels = render(&gpu, &scene, &camera, &inputs, &format!("{name}-{casts_shadow}"));
            let point = joy_renderer::project_to_viewport(shadowed_point, frame.view_projection(), size)
                .expect("shadowed floor is on screen");
            *color = pixel(&pixels, point.x as u32, point.y as u32);
        }
        let [unshadowed, shadowed] = colors;
        assert!(luminance(unshadowed) > 20.0, "{name}: floor should be lit without shadows: {unshadowed:?}");
        assert!(
            luminance(shadowed) < luminance(unshadowed) * 0.5,
            "{name}: blocker should shadow the floor: {shadowed:?} vs {unshadowed:?}"
        );
    }
}

#[test]
fn scene_nodes_move_and_hide_their_instances() {
    let Some(gpu) = gpu() else {
        return;
    };
    let mut scene = RenderScene::new();
    // The node starts far off-screen; its child sits at the node's origin.
    let away = SceneNode {
        transform: Transform::new(Vec3::new(500.0, 0.0, 0.0), Vec3::ZERO, Vec3::ONE),
        visible: true,
    };
    let node = scene.insert_node(away, None).unwrap();
    let crate_box = scene.create_mesh(unit_cube([1.0, 0.4, 0.1]));
    scene.set_parent(crate_box, Some(node)).unwrap();
    let camera = OrbitCamera {
        distance: 8.0,
        pitch: 0.6,
        ..OrbitCamera::default()
    };
    let inputs = FrameInputs {
        ambient: Some(1.0),
        ..FrameInputs::default()
    };
    let target = OffscreenTarget::new(&gpu, WIDTH, HEIGHT).expect("capture target");
    let mut renderer = Renderer::new(&gpu, joy_renderer::CAPTURE_FORMAT, RendererOptions::default());
    renderer.resize(WIDTH, HEIGHT, 1.0);
    renderer.render(&scene, &camera, &inputs, target.view()).unwrap();
    assert_eq!((renderer.diagnostics().visible, renderer.diagnostics().culled), (0, 1));

    scene.node_mut(node).unwrap().transform = Transform::IDENTITY;
    renderer.render(&scene, &camera, &inputs, target.view()).unwrap();
    assert_eq!(renderer.diagnostics().visible, 1, "moving the node brings its child into view");

    scene.node_mut(node).unwrap().visible = false;
    renderer.render(&scene, &camera, &inputs, target.view()).unwrap();
    assert_eq!((renderer.diagnostics().visible, renderer.diagnostics().culled), (0, 0), "hidden nodes hide children");
}

/// Profiling reports every phase a frame ran, CPU recording time always, and GPU time once timestamps read back on
/// devices that have them; turning it off clears the profile and stops recording.
#[test]
fn profiling_times_each_frame_phase() {
    let Some(gpu) = gpu() else {
        return;
    };
    let mut scene = RenderScene::new();
    scene.insert(MeshInstance::new(unit_cube([0.6, 0.6, 0.6])));
    let environment = noon();
    let inputs = FrameInputs {
        environment: Some(&environment),
        ..FrameInputs::default()
    };
    let target = OffscreenTarget::new(&gpu, WIDTH, HEIGHT).expect("capture target");
    let mut renderer = Renderer::new(&gpu, joy_renderer::CAPTURE_FORMAT, RendererOptions::default());
    renderer.resize(WIDTH, HEIGHT, 1.0);
    renderer.render(&scene, &OrbitCamera::default(), &inputs, target.view()).expect("frame renders");
    assert!(renderer.frame_profile().is_none(), "profiling starts off");

    renderer.set_profiling(true);
    let timestamps = gpu.capabilities().timestamps;
    let mut profile = None;
    for _ in 0..12 {
        renderer.render(&scene, &OrbitCamera::default(), &inputs, target.view()).expect("frame renders");
        // Waiting for the readback here only paces the test; the renderer itself never blocks on it.
        let _ = gpu.device().poll(wgpu::PollType::wait_indefinitely());
        profile = renderer.frame_profile().cloned();
        if profile.as_ref().is_some_and(|profile| profile.gpu_milliseconds.is_some() || !timestamps) {
            break;
        }
    }
    let profile = profile.expect("a profile completes");
    let phases: Vec<FramePhase> = profile.phases.iter().map(|timing| timing.phase).collect();
    let mut expected = vec![FramePhase::Prepare, FramePhase::Shadows, FramePhase::DepthPrepass];
    if renderer.diagnostics().deferred {
        expected.extend([FramePhase::GBuffer, FramePhase::Lighting]);
    }
    expected.extend([FramePhase::Scene, FramePhase::Post]);
    assert_eq!(phases, expected);
    assert!(profile.cpu_milliseconds > 0.0);
    if timestamps {
        assert!(profile.gpu_milliseconds.is_some_and(|total| total >= 0.0), "GPU time arrives: {profile:?}");
        assert!(profile.phases.iter().all(|timing| timing.gpu_milliseconds.is_some()));
    }

    renderer.set_profiling(false);
    renderer.render(&scene, &OrbitCamera::default(), &inputs, target.view()).expect("frame renders");
    assert!(renderer.frame_profile().is_none());
}
