// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scene effects on a real device: every effect compiles and renders across several frames, each changes the image
//! the way it should (and leaves it alone where it should), WebGL 2 falls back without volumetric fog, and invalid
//! configurations are refused.
//!
//! Needs a GPU adapter (llvmpipe or SwiftShader is enough); without one each test prints a note and returns. Set
//! `JOY_RENDER_CAPTURE_DIR` to keep PNG captures. Native only.
#![cfg(not(target_arch = "wasm32"))]

use joy_engine::core::LinearColor;
use joy_engine::core::Transform;
use joy_engine::development::{
    DevTools,
    EffectPreset,
    SceneEffectsControls,
    render_variables,
};
use joy_engine::math::{
    DVec3,
    Vec3,
};
use joy_engine::render_types::Mesh;
use joy_engine::rendering::scene_effects::{
    AdaptiveExposureOptions,
    MotionBlurOptions,
    SceneEffectsError,
    SceneEffectsOptions,
    TAA_NAME,
    TaaOptions,
    VOLUMETRICS_NAME,
    VolumetricOptions,
};
use joy_engine::rendering::{
    BackendPreference,
    CAPTURE_FORMAT,
    DebugView,
    DirectionalLight,
    EnvironmentLighting,
    FrameInputs,
    GpuBackend,
    GpuContext,
    MeshAsset,
    MeshInstance,
    MeshInstanceId,
    OffscreenTarget,
    OrbitCamera,
    PointLight,
    PostProcessSettings,
    PresentationTime,
    RenderScene,
    Renderer,
    RendererOptions,
    Skylight,
};

const WIDTH: u32 = 256;
const HEIGHT: u32 = 160;

fn gpu(preference: BackendPreference) -> Option<GpuContext> {
    match pollster::block_on(GpuContext::headless(preference)) {
        Ok(gpu) => Some(gpu),
        Err(error) => {
            eprintln!("skipping GPU test: {error}");
            None
        }
    }
}

fn cube(color: [f32; 3]) -> MeshAsset {
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
        let base = positions.len() as u32;
        let (normal, u, v) =
            (DVec3::from(normal.map(f64::from)), DVec3::from(u.map(f64::from)), DVec3::from(v.map(f64::from)));
        for (du, dv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            positions.push((normal + (u * du) + (v * dv)) * 0.5);
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let mesh = Mesh {
        colors: Some(vec![LinearColor::from(color); positions.len()]),
        ..Mesh::new(positions, indices)
    };
    MeshAsset::new(&mesh).expect("cube geometry is valid")
}

/// Midday light with the sun at `intensity` and the skylight scaled with it.
fn daylight(intensity: f32) -> EnvironmentLighting {
    let sun = DirectionalLight {
        direction: Vec3::new(0.35, 0.85, 0.4).normalize(),
        color: LinearColor::rgb(1.0, 0.96, 0.86),
        intensity,
        angular_radius: 0.00465,
        casts_shadow: true,
    };
    EnvironmentLighting {
        elapsed_seconds: 0.0,
        time_of_day: 0.5,
        sun,
        moon: DirectionalLight { direction: -sun.direction, intensity: 0.0, ..sun },
        primary: sun,
        skylight: Skylight::uniform(LinearColor::rgb(0.2, 0.42, 0.8), 0.09 * intensity),
        wind_velocity: Vec3::ZERO,
        scattering: [1.0, 0.6, 1.0, 1.0],
        fog_and_clouds: [0.002, 0.0, 0.0, 0.45],
        cloud_shape_and_shafts: [1800.0, 700.0, 0.65, 0.94],
        shaft_exposure_and_occlusion: [0.2, 1.0, 7.0],
    }
}

/// A ground slab with two crates; returns the scene and the moving crate.
fn scene() -> (RenderScene, MeshInstanceId) {
    let mut scene = RenderScene::new();
    scene.insert(MeshInstance::new(cube([0.31, 0.38, 0.39])).with_transform(Transform::new(
        Vec3::new(0.0, -0.25, 0.0),
        Vec3::ZERO,
        Vec3::new(30.0, 0.5, 30.0),
    )));
    let moving = scene.insert(MeshInstance::new(cube([0.95, 0.36, 0.12])).with_transform(Transform::new(
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::ZERO,
        Vec3::splat(2.0),
    )));
    scene.insert(MeshInstance::new(cube([0.2, 0.7, 0.5])).with_transform(Transform::new(
        Vec3::new(3.0, 0.75, -1.5),
        Vec3::new(0.0, 0.6, 0.0),
        Vec3::splat(1.5),
    )));
    (scene, moving)
}

/// One captured sequence: effects, frame count, crate speed, camera sway, sun intensity, and the clock's step.
#[derive(Clone, Copy)]
struct Shot {
    effects: Option<SceneEffectsOptions>,
    frames: u32,
    /// World units the crate moves along X each frame.
    crate_speed: f32,
    /// Whether the camera orbits slightly each frame.
    sway: bool,
    sun: f32,
    frame_seconds: f32,
}

impl Shot {
    /// A still scene in bright daylight at 60 frames per second.
    fn still(effects: Option<SceneEffectsOptions>, frames: u32) -> Self {
        Self {
            effects,
            frames,
            crate_speed: 0.0,
            sway: false,
            sun: 5.0,
            frame_seconds: 1.0 / 60.0,
        }
    }

    /// A moving crate and a swaying camera.
    fn moving(effects: Option<SceneEffectsOptions>, frames: u32) -> Self {
        Self {
            crate_speed: 0.1,
            sway: true,
            ..Self::still(effects, frames)
        }
    }
}

/// Render a shot; returns the last frame's pixels, saved as `name` when captures are kept.
fn render(gpu: &GpuContext, shot: Shot, name: &str) -> Vec<u8> {
    let (mut scene, moving) = scene();
    let target = OffscreenTarget::new(gpu, WIDTH, HEIGHT).expect("capture target");
    let options = RendererOptions {
        scene_effects: shot.effects,
        ..RendererOptions::default()
    };
    let mut renderer = Renderer::new(gpu, CAPTURE_FORMAT, options);
    renderer.resize(WIDTH, HEIGHT, 1.0);
    let environment = daylight(shot.sun);
    let lights = [PointLight {
        position: Vec3::new(-2.0, 2.0, 2.0),
        color: LinearColor::rgb(6.0, 3.0, 1.0),
        range: 8.0,
        casts_shadow: false,
    }];
    let mut pixels = Vec::new();
    for frame in 0..shot.frames {
        if let Some(instance) = scene.get_mut(moving) {
            instance.transform.position.x = frame as f32 * shot.crate_speed;
        }
        let sway = if shot.sway { frame as f32 / 300.0 } else { 0.0 };
        let camera = OrbitCamera {
            distance: 14.0,
            pitch: 0.55,
            yaw: 2.29 + sway,
            ..OrbitCamera::default()
        };
        let inputs = FrameInputs {
            environment: Some(&environment),
            lights: &lights,
            ..FrameInputs::default()
        };
        renderer.set_presentation_time(PresentationTime::fixed_rate(frame, shot.frame_seconds));
        renderer.render(&scene, &camera, &inputs, target.view()).expect("frame renders");
        pixels = target.read_rgba(gpu).expect("readback");
    }
    save(name, &pixels);
    pixels
}

fn save(name: &str, pixels: &[u8]) {
    if let Ok(directory) = std::env::var("JOY_RENDER_CAPTURE_DIR") {
        std::fs::create_dir_all(&directory).expect("capture directory");
        let path = std::path::Path::new(&directory).join(format!("{name}.png"));
        image::save_buffer(&path, pixels, WIDTH, HEIGHT, image::ColorType::Rgba8).expect("png written");
    }
}

/// Mean absolute per-channel difference between two captures.
fn difference(left: &[u8], right: &[u8]) -> f32 {
    let total: u64 = left.iter().zip(right).map(|(a, b)| u64::from(a.abs_diff(*b))).sum();
    total as f32 / left.len() as f32
}

/// Mean of the red, green, and blue channels.
fn brightness(pixels: &[u8]) -> f32 {
    let total: u64 = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| u64::from(pixel[0]) + u64::from(pixel[1]) + u64::from(pixel[2]))
        .sum();
    total as f32 / (pixels.len() / 4 * 3) as f32
}

/// Pixels whose color changed by more than a few levels between two captures.
fn changed_pixels(left: &[u8], right: &[u8]) -> usize {
    let (left, right) = (left.as_chunks::<4>().0, right.as_chunks::<4>().0);
    left.iter().zip(right).filter(|(a, b)| (0..3).any(|channel| a[channel].abs_diff(b[channel]) > 8)).count()
}

/// Pixels with any visible light.
fn lit_pixels(pixels: &[u8]) -> usize {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2]) > 30)
        .count()
}

fn everything() -> SceneEffectsOptions {
    SceneEffectsOptions {
        taa: Some(Default::default()),
        motion_blur: Some(Default::default()),
        adaptive_exposure: Some(Default::default()),
        volumetrics: Some(VolumetricOptions {
            density: 0.05,
            ..Default::default()
        }),
    }
}

#[test]
fn every_effect_renders_across_frames() {
    let Some(gpu) = gpu(BackendPreference::Auto) else {
        return;
    };
    let baseline = render(&gpu, Shot::moving(None, 6), "effects-off");
    let all = render(&gpu, Shot::moving(Some(everything()), 6), "effects-all");
    assert!(lit_pixels(&all) > (WIDTH * HEIGHT / 2) as usize, "most pixels stay lit");
    assert!(difference(&baseline, &all) > 0.5, "effects visibly change the frame");
}

/// History converges on a still scene: consecutive frames agree, and the antialiased image stays close to the
/// aliased one instead of drifting, darkening, or ghosting.
#[test]
fn temporal_antialiasing_converges_on_a_still_scene() {
    let Some(gpu) = gpu(BackendPreference::Auto) else {
        return;
    };
    let taa = Some(SceneEffectsOptions {
        taa: Some(Default::default()),
        ..Default::default()
    });
    let aliased = render(&gpu, Shot::still(None, 1), "taa-off");
    let settled = render(&gpu, Shot::still(taa, 24), "taa-settled");
    let next = render(&gpu, Shot::still(taa, 25), "taa-next");
    assert!(difference(&settled, &next) < 1.0, "a still image holds steady ({})", difference(&settled, &next));
    let change = difference(&aliased, &settled);
    assert!(change > 0.0 && change < 4.0, "antialiasing softens edges without changing the image ({change})");
    assert!((brightness(&settled) - brightness(&aliased)).abs() < 2.0, "history keeps the image's brightness");
}

/// Motion blur smears what moves and leaves a still frame untouched.
#[test]
fn motion_blur_follows_motion_only() {
    let Some(gpu) = gpu(BackendPreference::Auto) else {
        return;
    };
    let blur = Some(SceneEffectsOptions {
        motion_blur: Some(MotionBlurOptions {
            shutter: 1.0,
            max_length: 0.1,
            ..Default::default()
        }),
        ..Default::default()
    });
    let still = render(&gpu, Shot::still(None, 2), "blur-still-off");
    let still_blurred = render(&gpu, Shot::still(blur, 2), "blur-still-on");
    assert_eq!(changed_pixels(&still, &still_blurred), 0, "no motion, no blur");

    // About ten pixels of motion a frame, blurred along the crate's leading and trailing edges.
    let fast = |effects| Shot {
        crate_speed: 0.6,
        ..Shot::still(effects, 3)
    };
    let sharp = render(&gpu, fast(None), "blur-moving-off");
    let blurred = render(&gpu, fast(blur), "blur-moving-on");
    let smeared = changed_pixels(&sharp, &blurred);
    assert!(smeared > 300, "the moving crate smears ({smeared} pixels)");
}

/// Exposure brightens a dim scene and darkens a bright one toward middle gray over a few seconds.
#[test]
fn adaptive_exposure_moves_toward_middle_gray() {
    let Some(gpu) = gpu(BackendPreference::Auto) else {
        return;
    };
    let exposure = Some(SceneEffectsOptions {
        adaptive_exposure: Some(AdaptiveExposureOptions {
            brighten_speed: 3.0,
            ..Default::default()
        }),
        ..Default::default()
    });
    // Three seconds in tenth-second steps, inside the history's clock-jump limit.
    let shot = |effects, sun| Shot {
        sun,
        frame_seconds: 0.1,
        ..Shot::still(effects, 30)
    };
    let dim = render(&gpu, shot(None, 0.05), "exposure-dim-off");
    let dim_adapted = render(&gpu, shot(exposure, 0.05), "exposure-dim-on");
    let (dim, dim_adapted) = (brightness(&dim), brightness(&dim_adapted));
    assert!(dim_adapted > dim * 1.5, "a dim scene brightens ({dim} to {dim_adapted})");

    let bright = render(&gpu, shot(None, 20.0), "exposure-bright-off");
    let bright_adapted = render(&gpu, shot(exposure, 20.0), "exposure-bright-on");
    let (bright, bright_adapted) = (brightness(&bright), brightness(&bright_adapted));
    assert!(bright_adapted < bright * 0.8, "a bright scene darkens ({bright} to {bright_adapted})");
}

/// Fog thickens with density and keeps every pixel finite (no black holes from the integration).
#[test]
fn volumetric_fog_thickens_with_density() {
    let Some(gpu) = gpu(BackendPreference::Auto) else {
        return;
    };
    if !gpu.capabilities().compute {
        eprintln!("skipping volumetric test: no compute shaders");
        return;
    }
    let fog = |density| {
        Some(SceneEffectsOptions {
            volumetrics: Some(VolumetricOptions {
                density,
                height_falloff: 0.2,
                ..Default::default()
            }),
            ..Default::default()
        })
    };
    let clear = render(&gpu, Shot::still(None, 4), "fog-off");
    let thin = render(&gpu, Shot::still(fog(0.01), 4), "fog-thin");
    let thick = render(&gpu, Shot::still(fog(0.08), 4), "fog-thick");
    let (thin_change, thick_change) = (difference(&clear, &thin), difference(&clear, &thick));
    assert!(thin_change > 0.2 && thick_change > thin_change * 1.5, "fog thickens ({thin_change}, {thick_change})");
    assert!(lit_pixels(&thick) >= lit_pixels(&clear), "fogged pixels stay lit");
}

/// WebGL 2 (native OpenGL here) runs every effect but volumetric fog, which reports why.
#[test]
fn opengl_runs_every_effect_but_volumetrics() {
    let Some(gpu) = gpu(BackendPreference::WebGl) else {
        return;
    };
    if gpu.capabilities().backend != GpuBackend::Gl || !gpu.capabilities().hdr {
        eprintln!("skipping OpenGL test: no OpenGL adapter with float render targets");
        return;
    }
    let baseline = render(&gpu, Shot::moving(None, 4), "gl-effects-off");
    let all = render(&gpu, Shot::moving(Some(everything()), 4), "gl-effects-all");
    assert!(lit_pixels(&all) > (WIDTH * HEIGHT / 2) as usize);
    assert!(difference(&baseline, &all) > 0.5);

    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, RendererOptions::default());
    renderer.set_scene_effects(Some(everything())).unwrap();
    let report = renderer.scene_effects_capabilities();
    for effect in &report.effects {
        assert_eq!(effect.available, effect.name != VOLUMETRICS_NAME, "{}", effect.name);
    }
    let volumetrics = report.effects.iter().find(|effect| effect.name == VOLUMETRICS_NAME).unwrap();
    assert!(volumetrics.reason.is_some_and(|reason| reason.contains("compute")));
}

#[test]
fn invalid_effects_are_refused_and_capabilities_report_the_device() {
    let Some(gpu) = gpu(BackendPreference::Auto) else {
        return;
    };
    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, RendererOptions::default());
    let taa = SceneEffectsOptions {
        taa: Some(Default::default()),
        ..Default::default()
    };
    renderer.set_scene_effects(Some(taa)).unwrap();
    let invalid = SceneEffectsOptions {
        motion_blur: Some(MotionBlurOptions {
            sample_count: 0,
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(matches!(
        renderer.set_scene_effects(Some(invalid)),
        Err(SceneEffectsError::OutOfRange {
            setting: "sampleCount",
            ..
        })
    ));
    assert_eq!(renderer.scene_effects(), Some(&taa), "a refused configuration keeps the current effects");

    renderer.set_scene_effects(Some(everything())).unwrap();
    let report = renderer.scene_effects_capabilities();
    let compute = gpu.capabilities().compute;
    assert!(report.available);
    let running = report.effects.iter().filter(|effect| effect.requested && effect.available).count();
    assert_eq!(running, if compute { 4 } else { 3 });
    assert!(report.effects.iter().find(|effect| effect.name == TAA_NAME).unwrap().available);
    renderer.reset_history();
    renderer.set_scene_effects(None).unwrap();
    assert!(renderer.scene_effects().is_none());
}

/// The development preview controls sync from the renderer, commit presets to it, and layer their bloom choice over
/// the settings a game resolves each frame only once the author makes one.
#[test]
fn preview_controls_commit_presets_to_the_renderer() {
    let Some(gpu) = gpu(BackendPreference::Auto) else {
        return;
    };
    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, RendererOptions::default());
    let mut controls = SceneEffectsControls::new();
    controls.apply(&mut renderer);
    assert!(renderer.scene_effects().is_none(), "syncing must not touch the renderer");
    let resolved = PostProcessSettings { bloom_strength: 0.3, ..PostProcessSettings::DEFAULT };
    assert_eq!(controls.with_bloom_choice(resolved.clone()), resolved, "no choice yet passes settings through");

    controls.select_preset(EffectPreset::Atmosphere);
    controls.apply(&mut renderer);
    let effects = renderer.scene_effects().expect("preset applied");
    assert!(effects.volumetrics.is_some() && effects.adaptive_exposure.is_some() && effects.taa.is_none());
    let display = controls.with_bloom_choice(resolved.clone());
    assert_eq!(display.bloom_strength, 0.0, "named presets other than Combined clear bloom");

    controls.select_preset(EffectPreset::Combined);
    controls.apply(&mut renderer);
    assert!(renderer.scene_effects().unwrap().taa.is_some());
    assert!(controls.with_bloom_choice(resolved).bloom_strength > 0.0);

    // The committed effects render.
    let (scene, _) = scene();
    let target = OffscreenTarget::new(&gpu, WIDTH, HEIGHT).expect("capture target");
    renderer.resize(WIDTH, HEIGHT, 1.0);
    let environment = daylight(5.0);
    let inputs = FrameInputs {
        environment: Some(&environment),
        ..FrameInputs::default()
    };
    for _ in 0..2 {
        renderer.render(&scene, &OrbitCamera::default(), &inputs, target.view()).expect("frame renders");
    }
}

/// Render each effect alone for visual review (`JOY_RENDER_CAPTURE_DIR`); asserts only that each frame renders.
#[test]
fn each_effect_renders_alone() {
    let Some(gpu) = gpu(BackendPreference::Auto) else {
        return;
    };
    let all = everything();
    let single = [
        (
            "taa",
            SceneEffectsOptions {
                taa: all.taa,
                ..Default::default()
            },
        ),
        (
            "motion-blur",
            SceneEffectsOptions {
                motion_blur: all.motion_blur,
                ..Default::default()
            },
        ),
        (
            "exposure",
            SceneEffectsOptions {
                adaptive_exposure: all.adaptive_exposure,
                ..Default::default()
            },
        ),
        (
            "volumetrics",
            SceneEffectsOptions {
                volumetrics: all.volumetrics,
                ..Default::default()
            },
        ),
    ];
    for (name, options) in single {
        let pixels = render(&gpu, Shot::moving(Some(options), 4), &format!("effect-{name}"));
        assert_eq!(pixels.len(), (WIDTH * HEIGHT * 4) as usize);
    }
}

/// Console variables drive the renderer: `r.` settings apply, effect settings a game customized survive being turned
/// off and on, the variables mirror changes the game makes itself, and disabled development tools leave the renderer
/// alone.
#[test]
fn console_variables_drive_the_renderer() {
    let Some(gpu) = gpu(BackendPreference::Auto) else {
        return;
    };
    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, RendererOptions::default());
    let mut tools = DevTools::new("test", true);
    let commands =
        ["r.ScreenPercentage 50", "r.DebugView roughness", "r.AdaptiveExposure 1", "r.Profile 1", "r.Deferred 0"];
    for command in commands {
        tools.console().execute(command);
    }
    tools.apply_to_renderer(&mut renderer);
    assert_eq!(renderer.render_scale(), 0.5);
    assert_eq!(renderer.debug_view(), DebugView::Roughness);
    assert!(renderer.scene_effects().unwrap().adaptive_exposure.is_some() && renderer.profiling());
    assert!(!renderer.deferred(), "r.Deferred 0 shades forward");

    // The game customizes temporal antialiasing itself; the console reports it and restores it after an off and on.
    let custom = SceneEffectsOptions {
        taa: Some(TaaOptions {
            history_weight: 0.5,
        }),
        ..*renderer.scene_effects().unwrap()
    };
    renderer.set_scene_effects(Some(custom)).unwrap();
    tools.apply_to_renderer(&mut renderer);
    assert_eq!(tools.console_variables().bool(render_variables::TAA), Some(true));
    tools.console().execute("r.TAA 0");
    tools.apply_to_renderer(&mut renderer);
    assert!(renderer.scene_effects().unwrap().taa.is_none());
    tools.console().execute("r.TAA 1");
    tools.apply_to_renderer(&mut renderer);
    assert_eq!(
        renderer.scene_effects().unwrap().taa,
        Some(TaaOptions {
            history_weight: 0.5
        })
    );

    // Shadows park their options the same way.
    let shadows = renderer.shadow_options();
    tools.console().execute("r.Shadows 0");
    tools.apply_to_renderer(&mut renderer);
    assert!(renderer.shadow_options().is_none());
    tools.console().execute("r.Shadows 1");
    tools.apply_to_renderer(&mut renderer);
    assert_eq!(renderer.shadow_options(), shadows);

    // Production builds never apply console variables.
    let mut production = DevTools::new("production", false);
    production.console().execute("r.ScreenPercentage 25");
    production.apply_to_renderer(&mut renderer);
    assert_eq!(renderer.render_scale(), 0.5);
}
