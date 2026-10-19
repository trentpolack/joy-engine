// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scene effects on a real device: every effect compiles and renders across several frames (history, jitter, motion
//! vectors), visible effects change the image where expected, and invalid configurations are refused.
//!
//! Needs a GPU adapter (llvmpipe or SwiftShader is enough); without one each test prints a note and returns. Set
//! `JOY_RENDER_CAPTURE_DIR` to keep PNG captures. Native only.
#![cfg(not(target_arch = "wasm32"))]

use joy_engine::core::LinearColor;
use joy_engine::core::Transform;
use joy_engine::development::{
    EffectPreset,
    SceneEffectsControls,
};
use joy_engine::math::Vec3;
use joy_engine::rendering::scene_effects::{
    ClusteredLightingOptions,
    NormalSource,
    OutlineOptions,
    Reprojection,
    SceneEffectsError,
    SceneEffectsOptions,
    TaaOptions,
};
use joy_engine::rendering::{
    BackendPreference,
    CAPTURE_FORMAT,
    DirectionalLight,
    EnvironmentLighting,
    FrameInputs,
    GpuContext,
    MeshAsset,
    MeshChannels,
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

fn gpu() -> Option<GpuContext> {
    match pollster::block_on(GpuContext::headless(BackendPreference::Auto)) {
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
        let base = (positions.len() / 3) as u32;
        for (du, dv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            for axis in 0..3 {
                positions.push((normal[axis] + (u[axis] * du) + (v[axis] * dv)) * 0.5);
            }
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let colors: Vec<f32> = (0..positions.len() / 3).flat_map(|_| color).collect();
    MeshAsset::from_channels(MeshChannels {
        positions: &positions,
        indices: &indices,
        colors: &colors,
        ..Default::default()
    })
    .expect("cube geometry is valid")
}

fn noon() -> EnvironmentLighting {
    let sun = DirectionalLight {
        direction: Vec3::new(0.35, 0.85, 0.4).normalize(),
        color: LinearColor::rgb(1.0, 0.96, 0.86),
        intensity: 5.0,
        angular_radius: 0.00465,
        casts_shadow: true,
    };
    EnvironmentLighting {
        elapsed_seconds: 0.0,
        time_of_day: 0.5,
        sun,
        moon: DirectionalLight { direction: -sun.direction, intensity: 0.0, ..sun },
        primary: sun,
        skylight: Skylight::uniform(LinearColor::rgb(0.2, 0.42, 0.8), 0.45),
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
    let mut reflective = MeshInstance::new(cube([0.95, 0.36, 0.12])).with_transform(Transform::new(
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::ZERO,
        Vec3::splat(2.0),
    ));
    reflective.scene_roughness = 0.2;
    let moving = scene.insert(reflective);
    scene.insert(MeshInstance::new(cube([0.2, 0.7, 0.5])).with_transform(Transform::new(
        Vec3::new(3.0, 0.75, -1.5),
        Vec3::new(0.0, 0.6, 0.0),
        Vec3::splat(1.5),
    )));
    (scene, moving)
}

/// Render `frames` frames, moving a crate and the camera slightly between them; returns the last frame's pixels.
fn render(gpu: &GpuContext, effects: Option<SceneEffectsOptions>, frames: u32, name: &str) -> Vec<u8> {
    let (mut scene, moving) = scene();
    let target = OffscreenTarget::new(gpu, WIDTH, HEIGHT).expect("capture target");
    let options = RendererOptions { scene_effects: effects, ..RendererOptions::default() };
    let mut renderer = Renderer::new(gpu, CAPTURE_FORMAT, options);
    renderer.resize(WIDTH, HEIGHT, 1.0);
    let environment = noon();
    let lights = [PointLight {
        position: Vec3::new(-2.0, 2.0, 2.0),
        color: LinearColor::rgb(6.0, 3.0, 1.0),
        range: 8.0,
        casts_shadow: false,
    }];
    let mut pixels = Vec::new();
    for frame in 0..frames {
        let step = frame as f32 / 60.0;
        if let Some(instance) = scene.get_mut(moving) {
            instance.transform.position.x = step * 6.0;
        }
        let camera = OrbitCamera { distance: 14.0, pitch: 0.55, yaw: 2.29 + step * 0.2, ..OrbitCamera::default() };
        let inputs = FrameInputs { environment: Some(&environment), lights: &lights, ..FrameInputs::default() };
        renderer.set_presentation_time(PresentationTime::fixed_rate(frame, 1.0 / 60.0));
        renderer.render(&scene, &camera, &inputs, target.view()).expect("frame renders");
        pixels = target.read_rgba(gpu).expect("readback");
    }
    if let Ok(directory) = std::env::var("JOY_RENDER_CAPTURE_DIR") {
        std::fs::create_dir_all(&directory).expect("capture directory");
        let path = std::path::Path::new(&directory).join(format!("{name}.png"));
        image::save_buffer(&path, &pixels, WIDTH, HEIGHT, image::ColorType::Rgba8).expect("png written");
    }
    pixels
}

/// Mean absolute per-channel difference between two captures.
fn difference(left: &[u8], right: &[u8]) -> f32 {
    let total: u64 = left.iter().zip(right).map(|(a, b)| u64::from(a.abs_diff(*b))).sum();
    total as f32 / left.len() as f32
}

fn everything() -> SceneEffectsOptions {
    SceneEffectsOptions {
        depth_aware_blur: Some(Default::default()),
        gtao: Some(Default::default()),
        ssgi: Some(Default::default()),
        outlines: Some(Default::default()),
        taa: Some(Default::default()),
        motion_blur: Some(Default::default()),
        ssr: Some(Default::default()),
        height_fog: Some(Default::default()),
        clustered_lighting: Some(ClusteredLightingOptions { density: 0.2, ..Default::default() }),
        adaptive_exposure: Some(Default::default()),
        ..Default::default()
    }
}

#[test]
fn every_effect_renders_across_frames() {
    let Some(gpu) = gpu() else {
        return;
    };
    let baseline = render(&gpu, None, 4, "effects-off");
    let all = render(&gpu, Some(everything()), 4, "effects-all");
    let alternate = SceneEffectsOptions {
        ssao: Some(Default::default()),
        outlines: Some(OutlineOptions { normal_source: NormalSource::NormalTexture, ..Default::default() }),
        taa: Some(TaaOptions { reprojection: Reprojection::Camera, ..Default::default() }),
        ..Default::default()
    };
    let other = render(&gpu, Some(alternate), 4, "effects-ssao-camera-taa");
    for pixels in [&all, &other] {
        // The effects change the image without blanking it.
        let lit = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[0] as u32 + pixel[1] as u32 + pixel[2] as u32 > 30)
            .count();
        assert!(lit > (WIDTH * HEIGHT / 2) as usize, "most pixels stay lit");
        assert!(difference(&baseline, pixels) > 0.5, "effects visibly change the frame");
    }
}

#[test]
fn outlines_darken_silhouettes_only() {
    let Some(gpu) = gpu() else {
        return;
    };
    let baseline = render(&gpu, None, 1, "outlines-off");
    let outlined = render(
        &gpu,
        Some(SceneEffectsOptions {
            // Reconstructed normals are nearly flat at this distance and raw depth differs by less than the default
            // threshold, so silhouettes come from the normal attachment.
            outlines: Some(OutlineOptions { normal_source: NormalSource::NormalTexture, ..Default::default() }),
            ..Default::default()
        }),
        1,
        "outlines-on",
    );
    let mut darker = 0;
    let mut lighter = 0;
    for (before, after) in baseline.as_chunks::<4>().0.iter().zip(outlined.as_chunks::<4>().0) {
        let luminance = |pixel: &[u8]| u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2]);
        let (before, after) = (luminance(before), luminance(after));
        if after + 30 < before {
            darker += 1;
        } else if after > before + 30 {
            lighter += 1;
        }
    }
    assert!(darker > 50, "edges darken ({darker} pixels)");
    assert!(lighter < darker / 10, "outlines never brighten ({lighter} pixels)");
    assert!(darker < (WIDTH * HEIGHT / 4) as usize, "only edges change ({darker} pixels)");
}

#[test]
fn invalid_effects_are_refused_and_capabilities_report_the_backend() {
    let Some(gpu) = gpu() else {
        return;
    };
    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, RendererOptions::default());
    let both =
        SceneEffectsOptions { ssao: Some(Default::default()), gtao: Some(Default::default()), ..Default::default() };
    assert_eq!(renderer.set_scene_effects(Some(both)), Err(SceneEffectsError::ExclusiveOcclusion));
    assert!(renderer.scene_effects().is_none());
    renderer.set_scene_effects(Some(everything())).unwrap();
    let report = renderer.scene_effects_capabilities();
    assert!(report.available);
    assert_eq!(report.effects.iter().filter(|effect| effect.requested && effect.available).count(), 10);
    renderer.reset_history();
    renderer.set_scene_effects(None).unwrap();
    assert!(renderer.scene_volume_stats().is_none());
}

/// The development preview controls sync from the renderer, commit presets to it, and layer their bloom choice over
/// the settings a game resolves each frame only once the author makes one.
#[test]
fn preview_controls_commit_presets_to_the_renderer() {
    let Some(gpu) = gpu() else {
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
    assert!(
        effects.height_fog.is_some() && effects.clustered_lighting.is_some() && effects.adaptive_exposure.is_some()
    );
    let display = controls.with_bloom_choice(resolved.clone());
    assert_eq!(display.bloom_strength, 0.0, "named presets other than Combined clear bloom");

    controls.select_preset(EffectPreset::Combined);
    controls.apply(&mut renderer);
    assert!(renderer.scene_effects().unwrap().taa.is_some());
    assert!(controls.with_bloom_choice(resolved).bloom_strength > 0.0);

    // The committed chain renders.
    let (scene, _) = scene();
    let target = OffscreenTarget::new(&gpu, WIDTH, HEIGHT).expect("capture target");
    renderer.resize(WIDTH, HEIGHT, 1.0);
    let environment = noon();
    let inputs = FrameInputs { environment: Some(&environment), ..FrameInputs::default() };
    for _ in 0..2 {
        renderer.render(&scene, &OrbitCamera::default(), &inputs, target.view()).expect("frame renders");
    }
}

/// Render each effect alone for visual review (`JOY_RENDER_CAPTURE_DIR`); asserts only that each frame renders.
#[test]
fn each_effect_renders_alone() {
    let Some(gpu) = gpu() else {
        return;
    };
    let all = everything();
    let single = [
        ("blur", SceneEffectsOptions { depth_aware_blur: all.depth_aware_blur, ..Default::default() }),
        ("ssao", SceneEffectsOptions { ssao: Some(Default::default()), ..Default::default() }),
        ("gtao", SceneEffectsOptions { gtao: all.gtao, ..Default::default() }),
        ("ssgi", SceneEffectsOptions { ssgi: all.ssgi, ..Default::default() }),
        ("ssr", SceneEffectsOptions { ssr: all.ssr, ..Default::default() }),
        ("fog", SceneEffectsOptions { height_fog: all.height_fog, ..Default::default() }),
        ("clustered", SceneEffectsOptions { clustered_lighting: all.clustered_lighting, ..Default::default() }),
        ("taa", SceneEffectsOptions { taa: all.taa, ..Default::default() }),
        ("motion-blur", SceneEffectsOptions { motion_blur: all.motion_blur, ..Default::default() }),
        ("exposure", SceneEffectsOptions { adaptive_exposure: all.adaptive_exposure, ..Default::default() }),
    ];
    for (name, options) in single {
        let pixels = render(&gpu, Some(options), 4, &format!("effect-{name}"));
        assert_eq!(pixels.len(), (WIDTH * HEIGHT * 4) as usize);
    }
}
