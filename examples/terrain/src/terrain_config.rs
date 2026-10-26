// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Tuning for the terrain example. Demonstration units only; they are not the source's physical dimensions.

use joy_engine::core::{
    Color,
    LinearColor,
};
use joy_engine::math::Vec3;
use joy_engine::terrain::{
    TerrainCrater,
    TerrainDetail,
    TerrainDetailMaterial,
    TerrainStreamingBudget,
};

/// Camera control speeds: world units per second for flight, radians per CSS pixel for angles.
#[derive(Clone, Copy, Debug)]
pub struct CameraControlsConfig {
    /// Flight speed in world units per second.
    pub fly_speed: f32,
    /// Radians per CSS pixel while flying.
    pub look_sensitivity: f32,
    /// Radians per CSS pixel of horizontal orbit dragging.
    pub orbit_yaw_sensitivity: f32,
    /// Radians per CSS pixel of vertical orbit dragging.
    pub orbit_pitch_sensitivity: f32,
    /// Minimum orbit elevation in radians.
    pub min_orbit_pitch: f32,
    /// Maximum orbit elevation in radians.
    pub max_orbit_pitch: f32,
    /// Exponential distance change per scrolled CSS pixel.
    pub zoom_sensitivity: f32,
    /// Minimum orbit distance in world units.
    pub min_distance: f32,
    /// Maximum orbit distance in world units.
    pub max_distance: f32,
    /// Absolute flight pitch limit in radians, short of vertical.
    pub max_pitch: f32,
    /// CSS pixels per line for line-based wheel events.
    pub wheel_line_pixels: f32,
    /// Vertical field of view; must match the camera projection.
    pub vertical_fov_radians: f32,
}

/// Example tuning: LOD policy, presets, and the background.
#[derive(Clone, Copy, Debug)]
pub struct TerrainConfig {
    /// Default screen-space error tolerance in CSS pixels.
    pub tolerance: f32,
    /// Tolerance slider range and step in CSS pixels.
    pub tolerance_range: (f32, f32, f32),
    /// Stable-detail radius around the camera focus in world units.
    pub target_radius: f32,
    /// The page's Joy blue (#1b293d), sRGB-encoded; terrain materials stay local.
    pub background_color: Color,
    /// Overview orbit distance in world units.
    pub orbit_distance: f32,
    /// Overview framed radius in world units (drives the automatic far plane).
    pub orbit_radius: f32,
    /// Overview elevation in radians.
    pub orbit_pitch: f32,
    /// Overview heading in radians.
    pub orbit_yaw: f32,
    /// Overview focus; its height is replaced by the terrain height plus `target_height_offset`.
    pub target: Vec3,
    /// Overview focus height above the terrain in world units.
    pub target_height_offset: f32,
    /// Near-ground preset distance in world units.
    pub near_ground_distance: f32,
    /// Near-ground preset elevation in radians.
    pub near_ground_pitch: f32,
    /// Camera tour heading speed in radians per second.
    pub tour_yaw_speed: f32,
    /// Scene-renderer solar time in hours.
    pub scene_hours: f64,
    /// Camera controls.
    pub camera_controls: CameraControlsConfig,
    /// Paging and detail.
    pub streaming: StreamingConfig,
    /// Crater edits.
    pub crater: CraterConfig,
    /// Dropped physics props.
    pub props: PropConfig,
    /// Directional shadows.
    pub shadows: ShadowConfig,
}

/// How the example pages Dragon Pit and synthesizes detail below its authored two-unit spacing.
#[derive(Clone, Copy, Debug)]
pub struct StreamingConfig {
    /// Stored pyramid levels built from the embedded asset (five leaves one root tile).
    pub levels: u32,
    /// Polls before an embedded tile "arrives", so paging stays visible without a network.
    pub memory_latency_polls: u32,
    /// Residency and work limits.
    pub budget: TerrainStreamingBudget,
    /// Synthesized detail when CONFIG enables it.
    pub detail: TerrainDetail,
    /// Radius around the focus and every prop that keeps the collision level resident, in world units.
    pub collision_radius: f64,
}

/// Craters placed under the pointer with C.
#[derive(Clone, Copy, Debug)]
pub struct CraterConfig {
    /// The crater shape; its center is replaced by the picked point.
    pub shape: TerrainCrater,
    /// Farthest pick distance along the pointer ray, in world units.
    pub maximum_pick_distance: f64,
}

/// Props dropped onto the terrain with P.
#[derive(Clone, Copy, Debug)]
pub struct PropConfig {
    /// Props added per drop.
    pub burst: usize,
    /// Oldest props leave beyond this many.
    pub maximum: usize,
    /// Drop height above the terrain, in world units.
    pub drop_height: f32,
    /// Half-width of the square props scatter over around the focus, in world units.
    pub spread: f32,
    /// SDF crate half extent, in world units.
    pub crate_half_extent: f32,
    /// Sphere radius, in world units.
    pub ball_radius: f32,
    /// Mass of every prop in kilograms.
    pub mass: f32,
    /// SDF crate tint (linear).
    pub crate_color: LinearColor,
    /// Sphere tint (linear).
    pub ball_color: LinearColor,
    /// Seed of the scatter stream (cosmetic placement, never terrain data).
    pub scatter_seed: i64,
}

/// Directional shadows over the whole 2048-unit landscape, in world units.
#[derive(Clone, Copy, Debug)]
pub struct ShadowConfig {
    /// Camera view depth that receives shadows; covers the landscape from anywhere on it.
    pub distance: f32,
    /// Cascade split blend toward logarithmic spacing, so the first cascade stays near the camera (sharp enough for
    /// props) even though the last one reaches the far mountains.
    pub cascade_split_lambda: f32,
    /// Cascade map width and height in pixels.
    pub map_size: u32,
    /// Smallest receiver offset along the normal; cascades add their own texel-scaled offset on top.
    pub normal_bias: f32,
}

/// The example's tuning values.
pub const TERRAIN_CONFIG: TerrainConfig = TerrainConfig {
    tolerance: 2.0,
    tolerance_range: (0.5, 12.0, 0.5),
    target_radius: 90.0,
    background_color: Color::new(0.1058, 0.1607, 0.2392, 1.0),
    orbit_distance: 2300.0,
    orbit_radius: 1500.0,
    orbit_pitch: 0.52,
    orbit_yaw: 0.8,
    target: Vec3::new(0.0, 220.0, 0.0),
    target_height_offset: 12.0,
    near_ground_distance: 100.0,
    near_ground_pitch: 0.04,
    tour_yaw_speed: 0.2,
    scene_hours: 16.25,
    camera_controls: CameraControlsConfig {
        fly_speed: 150.0,
        look_sensitivity: 0.004,
        orbit_yaw_sensitivity: 0.005,
        orbit_pitch_sensitivity: 0.004,
        min_orbit_pitch: 0.01,
        max_orbit_pitch: 1.4,
        zoom_sensitivity: 0.001,
        min_distance: 5.0,
        max_distance: 10000.0,
        max_pitch: 1.5,
        wheel_line_pixels: 16.0,
        vertical_fov_radians: std::f32::consts::FRAC_PI_4,
    },
    streaming: StreamingConfig {
        levels: 5,
        memory_latency_polls: 2,
        budget: TerrainStreamingBudget {
            maximum_resident_tiles: 1536,
            maximum_pending_loads: 12,
            maximum_syntheses_per_update: 12,
        },
        detail: TerrainDetail {
            levels: 3,
            collision_level: -2,
            base_wavelength_samples: 4,
            octaves: 4,
            persistence: 0.5,
            ground: TerrainDetailMaterial {
                amplitude: 0.35,
                ridged: 0.0,
            },
            rock: TerrainDetailMaterial {
                amplitude: 1.0,
                ridged: 0.85,
            },
            snow: TerrainDetailMaterial {
                amplitude: 0.15,
                ridged: 0.0,
            },
            seed: 0x7E44_A1D5,
        },
        collision_radius: 48.0,
    },
    crater: CraterConfig {
        shape: TerrainCrater {
            center: [0.0, 0.0],
            radius: 7.0,
            depth: 3.5,
            rim_height: 1.0,
        },
        maximum_pick_distance: 8000.0,
    },
    props: PropConfig {
        burst: 6,
        maximum: 36,
        drop_height: 8.0,
        spread: 10.0,
        crate_half_extent: 1.1,
        ball_radius: 0.9,
        mass: 4.0,
        crate_color: LinearColor::rgb(0.86, 0.36, 0.12),
        ball_color: LinearColor::rgb(0.18, 0.42, 0.85),
        scatter_seed: 0x5CA7_7E25,
    },
    shadows: ShadowConfig {
        distance: 2500.0,
        cascade_split_lambda: 0.92,
        map_size: 2048,
        normal_bias: 0.05,
    },
};
