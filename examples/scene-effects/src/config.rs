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
    MotionBlurOptions,
    SceneEffectsOptions,
    TaaOptions,
    VolumetricOptions,
    VolumetricQuality,
};

/// What each effect shows in this room, in display order: label and detail.
pub const EFFECT_NOTES: [(&str, &str); 5] = [
    ("TAA", "Temporal antialiasing on thin slats and moving silhouettes."),
    ("Motion blur", "Enable object or camera motion to see blur along each surface's motion."),
    ("Adaptive exposure", "Exposure adapts toward the room's metered brightness before tone mapping."),
    ("Volumetric fog", "Height fog lit and shadowed by the point lights; needs compute shaders (not WebGL 2)."),
    ("HDR bloom", "Bright ceiling strip exceeds linear intensity 1."),
];

/// Volumetric fog in the room: thin near the floor and fading with height, scattering mostly forward so the lights
/// glow when the camera faces them.
const ROOM_FOG: VolumetricOptions = VolumetricOptions {
    density: 0.035,
    height_falloff: 0.35,
    base_height: 0.0,
    albedo: LinearColor::rgb(0.82, 0.88, 0.95),
    emissive: LinearColor::BLACK,
    anisotropy: 0.6,
    directional_intensity: 1.0,
    point_light_intensity: 1.0,
    ambient_intensity: 1.0,
    shadows: true,
    max_distance: 40.0,
    quality: VolumetricQuality::Medium,
    history_weight: 0.9,
};

/// Quality changes bounded sample counts and the fog volume's resolution; it never enables effects. Every entry is
/// complete, so the controls select whole presets per enabled effect.
pub fn quality_config(quality: EffectQuality) -> SceneEffectsOptions {
    let (blur_samples, fog_quality) = match quality {
        EffectQuality::Low => (8, VolumetricQuality::Low),
        EffectQuality::Medium => (12, VolumetricQuality::Medium),
        EffectQuality::High => (16, VolumetricQuality::High),
    };
    SceneEffectsOptions {
        taa: Some(TaaOptions {
            history_weight: 0.9,
        }),
        motion_blur: Some(MotionBlurOptions {
            shutter: 0.5,
            max_length: 0.04,
            sample_count: blur_samples,
        }),
        adaptive_exposure: Some(AdaptiveExposureOptions {
            minimum_exposure: 0.125,
            maximum_exposure: 4.0,
            initial_exposure: 1.0,
            ..Default::default()
        }),
        volumetrics: Some(VolumetricOptions {
            quality: fog_quality,
            ..ROOM_FOG
        }),
    }
}

/// Display settings: default AgX without bloom, so effects compare against a clean baseline. Bloom has threshold 1, so
/// only real HDR radiance (the emissive strips) blooms when enabled.
pub fn post_process_settings() -> PostProcessSettings {
    PostProcessSettings {
        tone_mapper: ToneMapper::Agx,
        agx_look: AgxLook::Default,
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
