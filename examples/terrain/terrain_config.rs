// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Tuning for the terrain example. Demonstration units only; they are not the source's physical dimensions.

use joy_engine::core::Color;
use joy_engine::math::Vec3;

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
};
