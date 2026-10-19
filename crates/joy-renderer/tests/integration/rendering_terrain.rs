// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Resident terrain on a real device: the standalone pass in every diagnostic mode, scene terrain in the lit pass with
//! directional shadows cast and received, scene-effect attachments, the WebGL-style CPU selection path on native
//! OpenGL when available, and refusal of invalid instance settings.
//!
//! Needs a GPU adapter (llvmpipe or lavapipe is enough); without one each test prints a note and returns. Set
//! `JOY_RENDER_CAPTURE_DIR` to keep PNG captures. Native only.
#![cfg(not(target_arch = "wasm32"))]

use joy_core::math::{
    DVec3,
    Vec3,
    deterministic,
};
use joy_core::{
    Color,
    LinearColor,
    Transform,
};
use joy_renderer::scene_effects::SceneEffectsOptions;
use joy_renderer::terrain::{
    TerrainAsset,
    TerrainDiagnostic,
    TerrainFrame,
    TerrainRenderer,
    TerrainTimingStatus,
    TerrainTriangleCount,
    TerrainView,
    parse_terrain,
    sample_terrain,
};
use joy_renderer::{
    BackendPreference,
    CAPTURE_FORMAT,
    Camera,
    CelestialLight,
    EnvironmentLighting,
    FrameInputs,
    GpuBackend,
    GpuContext,
    MeshAsset,
    MeshChannels,
    MeshInstance,
    OffscreenTarget,
    OrbitCamera,
    PointLight,
    RenderError,
    RenderScene,
    Renderer,
    RendererOptions,
    TerrainInstance,
    TerrainInstanceId,
};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 200;
/// The example's background (`#1b293d`), display-referred.
const BACKGROUND: Color = Color::new(0.1058, 0.1607, 0.2392, 1.0);
/// Uniform scale applied to the 2048-unit Dragon Pit in scene tests, so the default shadow distance covers it.
const SCENE_SCALE: f32 = 0.05;

fn gpu(preference: BackendPreference) -> Option<GpuContext> {
    match pollster::block_on(GpuContext::headless(preference)) {
        Ok(gpu) => Some(gpu),
        Err(error) => {
            eprintln!("skipping GPU test: {error}");
            None
        }
    }
}

fn dragon_pit() -> TerrainAsset {
    parse_terrain(
        include_str!("../../../../examples/terrain/assets/dragon-pit/terrain.json"),
        include_bytes!("../../../../examples/terrain/assets/dragon-pit/height.bin"),
    )
    .expect("the example asset is valid")
}

/// The example's overview camera.
fn overview(asset: &TerrainAsset) -> OrbitCamera {
    OrbitCamera {
        target: Vec3::new(0.0, sample_terrain(asset, 0.0, 0.0) as f32 + 12.0, 0.0),
        distance: 2300.0,
        radius: 1500.0,
        pitch: 0.52,
        yaw: 0.8,
        ..OrbitCamera::default()
    }
}

/// Standalone frame inputs for `camera` at the capture size (CSS pixels equal physical pixels here).
fn terrain_frame(camera: &OrbitCamera, diagnostic: TerrainDiagnostic, now_milliseconds: f64) -> TerrainFrame {
    let frame = camera.camera_frame(WIDTH as f32 / HEIGHT as f32);
    TerrainFrame {
        view_projection: frame.view_projection(),
        view: TerrainView {
            eye: frame.eye.as_dvec3(),
            projection_scale: f64::from(HEIGHT)
                / (2.0 * deterministic::tan(f64::from(camera.vertical_fov_radians) / 2.0)),
            tolerance: 2.0,
            target: camera.target.as_dvec3(),
            target_radius: 90.0,
        },
        diagnostic,
        clear_color: BACKGROUND,
        now_milliseconds,
    }
}

fn save(name: &str, pixels: &[u8]) {
    if let Ok(directory) = std::env::var("JOY_RENDER_CAPTURE_DIR") {
        std::fs::create_dir_all(&directory).expect("capture directory");
        let path = std::path::Path::new(&directory).join(format!("{name}.png"));
        image::save_buffer(&path, pixels, WIDTH, HEIGHT, image::ColorType::Rgba8).expect("png written");
    }
}

/// Fraction of pixels that differ from the background color.
fn coverage(pixels: &[u8]) -> f32 {
    let background = BACKGROUND.to_array().map(|channel| (channel * 255.0).round() as i32);
    let covered = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| (0..3).any(|channel| (i32::from(pixel[channel]) - background[channel]).abs() > 3))
        .count();
    covered as f32 / (WIDTH * HEIGHT) as f32
}

/// Mean absolute per-channel difference between two captures.
fn difference(left: &[u8], right: &[u8]) -> f32 {
    let total: u64 = left.iter().zip(right).map(|(a, b)| u64::from(a.abs_diff(*b))).sum();
    total as f32 / left.len() as f32
}

/// Render the standalone pass for `frames` frames and return the last frame's pixels.
fn render_standalone(
    gpu: &GpuContext,
    renderer: &mut TerrainRenderer,
    camera: &OrbitCamera,
    diagnostic: TerrainDiagnostic,
    frames: u32,
) -> Vec<u8> {
    let target = OffscreenTarget::new(gpu, WIDTH, HEIGHT).expect("capture target");
    for index in 0..frames {
        let mut encoder = gpu.device().create_command_encoder(&Default::default());
        let frame = terrain_frame(camera, diagnostic, f64::from(index) * 600.0);
        renderer.encode(&mut encoder, target.view(), (WIDTH, HEIGHT), &frame).expect("terrain frame");
        gpu.queue().submit([encoder.finish()]);
    }
    target.read_rgba(gpu).expect("readback")
}

#[test]
fn standalone_terrain_renders_every_diagnostic_mode() {
    let Some(gpu) = gpu(BackendPreference::Auto) else {
        return;
    };
    let asset = dragon_pit();
    let mut renderer = TerrainRenderer::new(&gpu, &asset, CAPTURE_FORMAT).expect("terrain renderer");
    let camera = overview(&asset);
    let material = render_standalone(&gpu, &mut renderer, &camera, TerrainDiagnostic::Material, 4);
    let lod = render_standalone(&gpu, &mut renderer, &camera, TerrainDiagnostic::LodColors, 1);
    let grid = render_standalone(&gpu, &mut renderer, &camera, TerrainDiagnostic::GridOverlay, 1);
    let ground = OrbitCamera { distance: 100.0, pitch: 0.04, ..camera };
    let near = render_standalone(&gpu, &mut renderer, &ground, TerrainDiagnostic::Material, 1);
    save("terrain-standalone-overview", &material);
    save("terrain-standalone-lod", &lod);
    save("terrain-standalone-grid", &grid);
    save("terrain-standalone-near-ground", &near);

    for (name, pixels) in [("material", &material), ("lod", &lod), ("grid", &grid), ("near", &near)] {
        assert!(coverage(pixels) > 0.35, "{name}: terrain covers the view ({})", coverage(pixels));
    }
    assert!(difference(&material, &lod) > 5.0, "LOD colors replace the material");
    assert!(difference(&lod, &grid) > 0.5, "the grid overlays the LOD colors");

    let stats = renderer.stats();
    assert_eq!(stats.patches, 256);
    assert_eq!(stats.max_triangles, 256 * 64 * 64 * 2);
    assert!(stats.resident_gpu_bytes > (1025 * 1025 * 4) as u64);
    assert_eq!(stats.resident_cpu_bytes, (1025 * 1025 * 2 + 65 * 65 * 8 + renderer_indices_bytes()) as u64);
    if renderer.uses_gpu_selection() {
        assert_eq!(stats.triangle_count_kind, TerrainTriangleCount::UpperBound);
        assert_eq!(stats.triangles, stats.max_triangles);
        assert!(renderer.selection().is_empty());
    } else {
        assert_eq!(stats.triangle_count_kind, TerrainTriangleCount::Selected);
        assert!(stats.triangles < stats.max_triangles);
    }
    assert!(!matches!(stats.gpu_timing, TerrainTimingStatus::Failed(_)), "timing: {}", stats.gpu_timing);
    if stats.gpu_timing == TerrainTimingStatus::Complete {
        assert!(stats.gpu_milliseconds.is_some_and(|milliseconds| milliseconds >= 0.0));
    }
}

/// Index bytes of the shared 64-cell lattice: six indices per cell at every stride.
fn renderer_indices_bytes() -> usize {
    (0..=6).map(|level| (64 >> level) * (64 >> level) * 6 * 4).sum()
}

fn low_sun() -> EnvironmentLighting {
    let sun = CelestialLight {
        direction: Vec3::new(0.75, 0.35, 0.45).normalize(),
        color: LinearColor::rgb(1.0, 0.94, 0.82),
        intensity: 4.5,
        angular_radius: 0.00465,
        casts_shadow: true,
    };
    EnvironmentLighting {
        elapsed_seconds: 0.0,
        time_of_day: 0.35,
        sun,
        moon: CelestialLight { direction: -sun.direction, intensity: 0.0, ..sun },
        primary: sun,
        skylight_color: LinearColor::rgb(0.2, 0.36, 0.62),
        skylight_intensity: 0.35,
        wind_velocity: Vec3::ZERO,
        scattering: [1.0, 0.6, 1.0, 1.0],
        fog_and_clouds: [0.002, 0.0, 0.0, 0.45],
        cloud_shape_and_shafts: [1800.0, 700.0, 0.65, 0.94],
        shaft_exposure_and_occlusion: [0.2, 1.0, 7.0],
    }
}

fn crate_box() -> MeshAsset {
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    for (normal, u, v) in [
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ([-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]),
        ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        ([0.0, -1.0, 0.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ] {
        let base = (positions.len() / 3) as u32;
        for (du, dv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            for axis in 0..3 {
                positions.push((normal[axis] + (u[axis] * du) + (v[axis] * dv)) * 0.5);
            }
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let colors: Vec<f32> = (0..positions.len() / 3).flat_map(|_| [0.95_f32, 0.4, 0.15]).collect();
    MeshAsset::from_channels(MeshChannels {
        positions: &positions,
        indices: &indices,
        colors: &colors,
        ..Default::default()
    })
    .expect("box geometry is valid")
}

/// Dragon Pit scaled into a 102-unit landscape with a crate hovering over the focus.
fn scene(asset: &TerrainAsset) -> (RenderScene, TerrainInstanceId, Vec3) {
    let mut scene = RenderScene::new();
    let terrain = scene.insert_terrain(TerrainInstance::new(asset.clone()).with_transform(Transform::new(
        Vec3::ZERO,
        Vec3::ZERO,
        Vec3::splat(SCENE_SCALE),
    )));
    let ground = sample_terrain(asset, 0.0, 0.0) as f32 * SCENE_SCALE;
    let focus = Vec3::new(0.0, ground, 0.0);
    scene.insert(MeshInstance::new(crate_box()).with_transform(Transform::new(
        focus + Vec3::new(0.0, 6.0, 0.0),
        Vec3::new(0.0, 0.5, 0.0),
        Vec3::splat(4.0),
    )));
    (scene, terrain, focus)
}

fn scene_camera(focus: Vec3, yaw: f32) -> OrbitCamera {
    OrbitCamera { target: focus, distance: 70.0, radius: 60.0, pitch: 0.5, yaw, ..OrbitCamera::default() }
}

/// Render `frames` scene frames (the camera turns slightly each frame) and return the last frame's pixels.
fn render_scene(
    gpu: &GpuContext,
    renderer: &mut Renderer,
    scene: &RenderScene,
    focus: Vec3,
    frames: u32,
) -> Result<Vec<u8>, RenderError> {
    let target = OffscreenTarget::new(gpu, WIDTH, HEIGHT).expect("capture target");
    let environment = low_sun();
    for frame in 0..frames {
        let step = frame as f32 / 60.0;
        let inputs = FrameInputs { environment: Some(&environment), time_seconds: step, ..FrameInputs::default() };
        renderer.render(scene, &scene_camera(focus, 0.9 + step * 0.2), &inputs, target.view())?;
    }
    Ok(target.read_rgba(gpu).expect("readback"))
}

fn scene_renderer(gpu: &GpuContext, effects: Option<SceneEffectsOptions>) -> Renderer {
    let mut renderer =
        Renderer::new(gpu, CAPTURE_FORMAT, RendererOptions { scene_effects: effects, ..RendererOptions::default() });
    renderer.resize(WIDTH, HEIGHT, 1.0);
    renderer
}

#[test]
fn scene_terrain_is_lit_and_casts_and_receives_shadows() {
    let Some(gpu) = gpu(BackendPreference::Auto) else {
        return;
    };
    let asset = dragon_pit();
    let (mut scene, terrain, focus) = scene(&asset);
    let mut renderer = scene_renderer(&gpu, None);

    let lit = render_scene(&gpu, &mut renderer, &scene, focus, 2).expect("frame renders");
    let diagnostics = renderer.diagnostics().clone();
    assert_eq!(diagnostics.visible, 2, "terrain and crate are visible");
    assert!(diagnostics.shadows_active);
    assert!(diagnostics.draw_calls >= 256 * 2, "terrain draws every patch into the shadow map and scene");
    save("terrain-scene-lit", &lit);

    scene.terrain_mut(terrain).unwrap().casts_shadow = false;
    let no_cast = render_scene(&gpu, &mut renderer, &scene, focus, 1).expect("frame renders");
    save("terrain-scene-no-cast", &no_cast);
    scene.terrain_mut(terrain).unwrap().casts_shadow = true;
    scene.terrain_mut(terrain).unwrap().receives_shadow = false;
    let no_receive = render_scene(&gpu, &mut renderer, &scene, focus, 1).expect("frame renders");
    save("terrain-scene-no-receive", &no_receive);
    scene.terrain_mut(terrain).unwrap().receives_shadow = true;
    scene.terrain_mut(terrain).unwrap().diagnostic = TerrainDiagnostic::GridOverlay;
    let grid = render_scene(&gpu, &mut renderer, &scene, focus, 1).expect("frame renders");
    save("terrain-scene-grid", &grid);
    scene.remove_terrain(terrain);
    let removed = render_scene(&gpu, &mut renderer, &scene, focus, 1).expect("frame renders");
    save("terrain-scene-removed", &removed);
    assert_eq!(renderer.diagnostics().visible, 1);

    assert!(difference(&lit, &removed) > 10.0, "terrain fills the lit view");
    assert!(difference(&lit, &no_cast) > 0.2, "terrain casts shadows (onto itself)");
    assert!(difference(&lit, &no_receive) > 0.2, "terrain receives shadows");
    assert!(difference(&lit, &grid) > 2.0, "scene terrain honors its diagnostic mode");
}

/// Terrain draws into every shadow view, including the six faces of a shadowed point light.
#[test]
fn scene_terrain_casts_into_point_light_faces() {
    let Some(gpu) = gpu(BackendPreference::Auto) else {
        return;
    };
    let asset = dragon_pit();
    let (mut scene, terrain, focus) = scene(&asset);
    let mut renderer = scene_renderer(&gpu, None);
    let target = OffscreenTarget::new(&gpu, WIDTH, HEIGHT).expect("capture target");
    let lights = [PointLight {
        position: focus + Vec3::new(-14.0, 3.0, -10.0),
        color: LinearColor::rgb(40.0, 22.0, 9.0),
        range: 45.0,
        casts_shadow: true,
    }];
    let inputs = FrameInputs { lights: &lights, ..FrameInputs::default() };
    let mut render = |scene: &RenderScene| {
        renderer.render(scene, &scene_camera(focus, 0.9), &inputs, target.view()).expect("frame renders");
        (target.read_rgba(&gpu).expect("readback"), renderer.diagnostics().clone())
    };
    let (casting, diagnostics) = render(&scene);
    assert_eq!(diagnostics.shadow_point_lights, 1);
    assert_eq!(diagnostics.shadow_views, 6, "no environment, so only the point light's faces");
    assert!(diagnostics.draw_calls >= 256 * 7, "terrain draws into six faces and the scene pass");
    save("terrain-scene-point-light", &casting);
    scene.terrain_mut(terrain).unwrap().casts_shadow = false;
    let (open, _) = render(&scene);
    save("terrain-scene-point-light-no-cast", &open);
    assert!(difference(&casting, &open) > 0.2, "terrain occludes the point light");
}

#[test]
fn scene_terrain_draws_into_effect_attachments() {
    let Some(gpu) = gpu(BackendPreference::Auto) else {
        return;
    };
    let asset = dragon_pit();
    let (mut scene, terrain, focus) = scene(&asset);
    let everything = SceneEffectsOptions {
        gtao: Some(Default::default()),
        outlines: Some(Default::default()),
        taa: Some(Default::default()),
        motion_blur: Some(Default::default()),
        ssr: Some(Default::default()),
        height_fog: Some(Default::default()),
        ..Default::default()
    };
    let mut plain = scene_renderer(&gpu, None);
    let mut effects = scene_renderer(&gpu, Some(everything));
    let baseline = render_scene(&gpu, &mut plain, &scene, focus, 3).expect("frame renders");

    // Move the terrain between frames so its motion vectors include transform history.
    let target = OffscreenTarget::new(&gpu, WIDTH, HEIGHT).expect("capture target");
    let environment = low_sun();
    for frame in 0..4 {
        let step = frame as f32 / 60.0;
        scene.terrain_mut(terrain).unwrap().transform.position.x = step * 3.0;
        let inputs = FrameInputs { environment: Some(&environment), time_seconds: step, ..FrameInputs::default() };
        effects.render(&scene, &scene_camera(focus, 0.9 + step * 0.2), &inputs, target.view()).expect("frame renders");
    }
    let processed = target.read_rgba(&gpu).expect("readback");
    save("terrain-scene-effects", &processed);
    let lit = processed
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2]) > 30);
    assert!(lit.count() > (WIDTH * HEIGHT / 2) as usize, "effects keep the terrain visible");
    assert!(difference(&baseline, &processed) > 0.5, "effects change the terrain frame");
}

#[test]
fn invalid_terrain_settings_are_refused() {
    let Some(gpu) = gpu(BackendPreference::Auto) else {
        return;
    };
    let asset = dragon_pit();
    let (mut scene, terrain, focus) = scene(&asset);
    let mut renderer = scene_renderer(&gpu, None);
    scene.terrain_mut(terrain).unwrap().tolerance = 0.0;
    assert!(matches!(render_scene(&gpu, &mut renderer, &scene, focus, 1), Err(RenderError::Terrain(_))));
    scene.terrain_mut(terrain).unwrap().tolerance = 2.0;
    scene.terrain_mut(terrain).unwrap().target_radius = f32::NAN;
    assert!(matches!(render_scene(&gpu, &mut renderer, &scene, focus, 1), Err(RenderError::Terrain(_))));
    scene.terrain_mut(terrain).unwrap().target_radius = 0.0;
    scene.terrain_mut(terrain).unwrap().transform.scale = Vec3::new(1.0, f32::NAN, 1.0);
    assert!(matches!(render_scene(&gpu, &mut renderer, &scene, focus, 1), Err(RenderError::Terrain(_))));
    scene.terrain_mut(terrain).unwrap().transform.scale = Vec3::ZERO;
    render_scene(&gpu, &mut renderer, &scene, focus, 1).expect("a collapsed transform pins the finest geometry");
    scene.terrain_mut(terrain).unwrap().transform.scale = Vec3::splat(SCENE_SCALE);
    render_scene(&gpu, &mut renderer, &scene, focus, 1).expect("valid settings render again");

    let mut standalone = TerrainRenderer::new(&gpu, &asset, CAPTURE_FORMAT).expect("terrain renderer");
    let mut frame = terrain_frame(&overview(&asset), TerrainDiagnostic::Material, 0.0);
    frame.view.eye = DVec3::new(0.0, f64::NAN, 0.0);
    let target = OffscreenTarget::new(&gpu, WIDTH, HEIGHT).expect("capture target");
    let mut encoder = gpu.device().create_command_encoder(&Default::default());
    assert!(standalone.encode(&mut encoder, target.view(), (WIDTH, HEIGHT), &frame).is_err());
}

/// WebGL 2 selects LODs on the CPU; native OpenGL takes the same path, so run both draw paths there when available.
#[test]
fn opengl_selects_on_the_cpu_and_renders_both_paths() {
    let Some(gpu) = gpu(BackendPreference::WebGl) else {
        return;
    };
    if gpu.capabilities().backend != GpuBackend::Gl {
        eprintln!("skipping OpenGL test: no OpenGL adapter");
        return;
    }
    let asset = dragon_pit();
    let mut renderer = TerrainRenderer::new(&gpu, &asset, CAPTURE_FORMAT).expect("terrain renderer");
    assert!(!renderer.uses_gpu_selection());
    let pixels = render_standalone(&gpu, &mut renderer, &overview(&asset), TerrainDiagnostic::LodColors, 2);
    save("terrain-gl-standalone-lod", &pixels);
    assert!(coverage(&pixels) > 0.35);
    assert_eq!(renderer.selection().len(), 256);
    assert_eq!(renderer.stats().triangle_count_kind, TerrainTriangleCount::Selected);
    assert!(renderer.stats().triangles < renderer.stats().max_triangles);
    assert_eq!(renderer.stats().frame_upload_bytes, 128 + 256 * 256);

    let (scene, _, focus) = scene(&asset);
    let mut scene_renderer = scene_renderer(&gpu, None);
    let lit = render_scene(&gpu, &mut scene_renderer, &scene, focus, 2).expect("frame renders");
    save("terrain-gl-scene-lit", &lit);
    assert!(scene_renderer.diagnostics().shadows_active);
    assert!(lit.as_chunks::<4>().0.iter().filter(|pixel| pixel[1] > 40).count() > (WIDTH * HEIGHT / 3) as usize);
}
