// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scene effects lab configuration: effect notes, the per-quality effect tuning, and the view, lighting, bloom, and
//! motion values the lab renders with. Distances are world units and times are seconds.

use joy_engine::development::EffectQuality;
use joy_engine::prelude::*;
use joy_engine::rendering::post_process::{
    AgxLook,
    BloomQuality,
    ToneMapper,
};
use joy_engine::rendering::scene_effects::{
    AdaptiveExposureOptions,
    ClusteredLightingOptions,
    DepthAwareBlurOptions,
    GtaoOptions,
    HeightFogOptions,
    MotionBlurOptions,
    OutlineOptions,
    Reprojection,
    SceneEffectsOptions,
    SsaoOptions,
    SsgiOptions,
    SsrOptions,
    TaaOptions,
};

/// What each effect shows in this room, in display order: label and detail.
pub const EFFECT_NOTES: [(&str, &str); 12] = [
    ("Depth-aware blur", "Softens surfaces while preserving depth edges."),
    ("SSAO", "Contact darkening near blocks and wall corners."),
    ("GTAO", "Alternative horizon-based ambient occlusion; replaces SSAO."),
    ("SSGI", "Screen-space color bleed from the red and green walls."),
    ("Outlines", "Depth and normal discontinuities around geometry."),
    ("TAA", "Temporal antialiasing on thin slats and moving silhouettes."),
    ("Motion blur", "Enable object or camera motion to see velocity trails."),
    ("SSR", "Screen-space reflection on the smooth floor and blue block."),
    ("Height fog", "Stylized screen-height approximation using scene depth."),
    ("Clustered lighting", "Volumetric scattering from clustered point-light lists."),
    ("Adaptive exposure", "Temporal luminance adaptation before display tone mapping."),
    ("HDR bloom", "Bright ceiling strip exceeds linear intensity 1."),
];

/// Quality changes bounded sample counts and effect resolution; it never enables effects. Every entry is complete, so
/// the controls select whole presets per enabled effect.
pub fn quality_config(quality: EffectQuality) -> SceneEffectsOptions {
    let resolution_scale = if quality == EffectQuality::High { 1.0 } else { 0.5 };
    let pick = |low: f32, medium: f32, high: f32| match quality {
        EffectQuality::Low => low,
        EffectQuality::Medium => medium,
        EffectQuality::High => high,
    };
    SceneEffectsOptions {
        depth_aware_blur: Some(DepthAwareBlurOptions { radius: pick(2.0, 4.0, 4.0), ..Default::default() }),
        ssao: Some(SsaoOptions { radius: 4.0, intensity: 1.5, resolution_scale, ..Default::default() }),
        gtao: Some(GtaoOptions { radius: 2.2, intensity: 3.2, resolution_scale, ..Default::default() }),
        ssgi: Some(SsgiOptions {
            resolution_scale,
            ray_count: pick(3.0, 7.0, 12.0),
            step_count: pick(8.0, 8.0, 12.0),
            ..Default::default()
        }),
        outlines: Some(OutlineOptions {
            color: LinearColor::new(0.035, 0.04, 0.07, 1.0),
            thickness: 1.5,
            ..Default::default()
        }),
        taa: Some(TaaOptions { history_weight: 0.9, reprojection: Reprojection::Velocity, ..Default::default() }),
        motion_blur: Some(MotionBlurOptions { strength: 1.5, sample_count: pick(6.0, 10.0, 16.0) }),
        ssr: Some(SsrOptions {
            resolution_scale,
            sample_count: pick(16.0, 48.0, 80.0),
            max_roughness: 0.7,
            ..Default::default()
        }),
        height_fog: Some(HeightFogOptions {
            density: 0.22,
            height_falloff: 3.0,
            fog_color: LinearColor::new(0.18, 0.34, 0.48, 0.6),
            ..Default::default()
        }),
        clustered_lighting: Some(ClusteredLightingOptions {
            resolution_scale,
            sample_count: pick(5.0, 10.0, 16.0),
            ..Default::default()
        }),
        adaptive_exposure: Some(AdaptiveExposureOptions {
            minimum_exposure: 0.125,
            maximum_exposure: 4.0,
            initial_exposure: 1.0,
            ..Default::default()
        }),
        color_format: None,
    }
}

/// Display settings: AgX neutral without antialiasing or bloom, so effects compare against a clean baseline. Bloom has
/// threshold 1, so only real HDR radiance (the emissive strips) blooms when enabled.
pub fn post_process_settings() -> PostProcessSettings {
    PostProcessSettings {
        tone_mapper: ToneMapper::Agx,
        agx_look: AgxLook::Neutral,
        antialias_strength: 0.0,
        bloom_strength: 0.0,
        bloom_threshold: 1.0,
        bloom_radius: 0.035,
        bloom_quality: BloomQuality::Medium,
        exposure: 1.0,
        ..PostProcessSettings::DEFAULT
    }
}

/// Renderer settings: the display baseline above, default shadows, and scene effects created off, so the first frame is
/// the comparison baseline.
pub fn renderer_options() -> RendererOptions {
    RendererOptions {
        post_process: post_process_settings(),
        scene_effects: Some(SceneEffectsOptions::default()),
        ..RendererOptions::default()
    }
}

/// Bloom strength the HDR bloom toggle applies.
pub const BLOOM_STRENGTH: f32 = 0.65;

/// Orbit camera framing: target, content radius, distance, clips, and default heading and elevation (radians).
pub const CAMERA_TARGET: Vec3 = Vec3::new(0.0, 2.0, -0.6);
/// Framed content radius.
pub const CAMERA_RADIUS: f32 = 12.0;
/// Starting eye distance.
pub const CAMERA_DISTANCE: f32 = 20.0;
/// Near and far clips that keep useful depth precision in this small room.
pub const CAMERA_CLIPS: (f32, f32) = (0.5, 100.0);
/// Starting heading.
pub const CAMERA_YAW: f32 = 0.12;
/// Starting elevation.
pub const CAMERA_PITCH: f32 = 0.25;
/// Elevation limits while orbiting.
pub const CAMERA_PITCH_LIMITS: (f32, f32) = (-0.1, 1.2);
/// Zoom limits.
pub const CAMERA_DISTANCE_LIMITS: (f32, f32) = (8.0, 32.0);
/// Radians of orbit per logical pixel of drag.
pub const ORBIT_SENSITIVITY: f32 = 0.004;
/// Zoom factor exponent per logical pixel of wheel scroll.
pub const ZOOM_SENSITIVITY: f32 = 0.001;
/// Heading offset a camera cut toggles.
pub const CAMERA_CUT_YAW: f32 = -0.42;

/// Ambient irradiance and the linear background color.
pub const AMBIENT: f32 = 0.18;
/// Linear background color behind the open room.
pub const CLEAR_COLOR: LinearColor = LinearColor::rgb(0.045, 0.055, 0.08);

/// Simulation starts frozen here, while frames keep rendering to accumulate temporal history.
pub const START_SECONDS: f64 = 2.0;
/// The manual step and the frozen clock's delta.
pub const STEP_SECONDS: f64 = 1.0 / 60.0;
/// Largest simulated step while the clock runs.
pub const MAXIMUM_STEP_SECONDS: f64 = 0.05;
/// Moving block: sideways amplitude, its angular rate (radians per second), and its spin rate.
pub const OBJECT_MOTION: (f64, f64, f64) = (2.6, 1.4, 0.9);
/// Camera sway: heading amplitude and angular rate.
pub const CAMERA_MOTION: (f64, f64) = (0.2, 0.5);

/// Frames between HUD statistics refreshes, and the samples each timing distribution keeps.
pub const STATS_INTERVAL_FRAMES: u64 = 15;
/// Timing samples retained per distribution.
pub const TIMING_SAMPLES: usize = 240;
