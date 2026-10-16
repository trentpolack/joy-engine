// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM destruction tuning (TypeScript `config/destruction-config.ts`). Dimensions are meters, gravity meters per
//! second squared, launch velocities meters per second, impulses newton-seconds, angles radians, times seconds, and
//! drag distances logical pixels. Colors are linear RGB.

use joy_engine::physics::ChunkSupportOptions;
use joy_engine::prelude::*;

/// Structure generation, damage, and debris physics.
pub struct DestructionConfig {
    /// Seed for material variation; identities and collision never depend on it.
    pub seed: i64,
    /// Front-wall chunk dimensions.
    pub chunk_size: [f64; 3],
    /// Front-wall columns and rows.
    pub wall_columns: u32,
    /// Front-wall rows.
    pub wall_rows: u32,
    /// Starting resistance of solid masonry, in damage units.
    pub hit_points: f64,
    /// Starting resistance of the stronger return-wall frame.
    pub frame_hit_points: f64,
    /// Default damage sphere radius.
    pub damage_radius: f64,
    /// Default damage per impact.
    pub damage_per_hit: f64,
    /// Longest aimed impact.
    pub maximum_aim_distance: f64,
    /// Downward debris acceleration; deliberately brisk.
    pub debris_gravity: f32,
    /// Debris body mass in kilograms.
    pub debris_mass: f32,
    /// Size fraction a directly fractured chunk keeps, in mesh and collider alike.
    pub fractured_chunk_scale: f64,
    /// Debris friction.
    pub debris_friction: f32,
    /// Debris restitution.
    pub debris_restitution: f32,
    /// Debris linear damping per second.
    pub debris_linear_damping: f32,
    /// Debris angular damping per second.
    pub debris_angular_damping: f32,
    /// Launch velocity of a fractured chunk: outward X, up, and toward the camera.
    pub debris_launch_speed: [f32; 3],
    /// Per-piece launch variation added to X and Y.
    pub debris_launch_variation: [f32; 2],
    /// Spin impulse applied at the top of a fractured chunk.
    pub debris_spin_impulse: [f32; 3],
    /// Fixed physics step.
    pub physics_step_seconds: f32,
    /// Most physics steps per frame.
    pub maximum_physics_substeps: u32,
}

/// The example's structure, damage, and debris tuning.
pub const DESTRUCTION: DestructionConfig = DestructionConfig {
    seed: 7319,
    chunk_size: [1.0, 1.0, 0.55],
    wall_columns: 13,
    wall_rows: 6,
    hit_points: 6.0,
    frame_hit_points: 10.0,
    damage_radius: 1.15,
    damage_per_hit: 1.0,
    maximum_aim_distance: 60.0,
    debris_gravity: 24.0,
    debris_mass: 1.0,
    fractured_chunk_scale: 0.9,
    debris_friction: 0.7,
    debris_restitution: 0.08,
    debris_linear_damping: 0.12,
    debris_angular_damping: 0.25,
    debris_launch_speed: [0.8, 0.25, 1.8],
    debris_launch_variation: [0.35, 0.08],
    debris_spin_impulse: [0.2, 0.0, 0.25],
    physics_step_seconds: 1.0 / 120.0,
    maximum_physics_substeps: 6,
};

/// The ground slab; the rendered slab and its static collider share these values.
pub struct GroundConfig {
    /// Slab center.
    pub center: [f64; 3],
    /// Full slab dimensions; its top is at world Y = 0.
    pub size: [f64; 3],
    /// Neutral slab tint, independent of the chunk material palette.
    pub tint: [f32; 3],
    /// Ground friction.
    pub friction: f32,
    /// Ground restitution.
    pub restitution: f32,
}

/// The example's ground.
pub const GROUND: GroundConfig = GroundConfig {
    center: [0.0, -1.0, 0.0],
    size: [200.0, 2.0, 200.0],
    tint: [0.3, 0.36, 0.42],
    friction: 0.8,
    restitution: 0.08,
};

/// Ground support: grounded masonry can bridge one neighboring chunk; longer unsupported spans detach.
pub struct SupportConfig {
    /// World height of the ground anchors (the slab's top).
    pub ground_height: f64,
    /// Longest sideways travel from an anchor.
    pub maximum_cantilever_distance: f64,
    /// Face contact tolerance.
    pub contact_tolerance: f64,
    /// SUPPORT view color of chunks resting on the ground.
    pub anchored_color: [f64; 3],
    /// SUPPORT view color of chunks held up vertically.
    pub vertical_color: [f64; 3],
    /// SUPPORT view color of chunks braced sideways.
    pub braced_color: [f64; 3],
}

impl SupportConfig {
    /// The engine support rule these values configure.
    pub fn options(&self) -> ChunkSupportOptions {
        ChunkSupportOptions {
            ground_height: self.ground_height,
            maximum_cantilever_distance: self.maximum_cantilever_distance,
            contact_tolerance: Some(self.contact_tolerance),
        }
    }
}

/// The example's support rule.
pub const SUPPORT: SupportConfig = SupportConfig {
    ground_height: GROUND.center[1] + (GROUND.size[1] / 2.0),
    maximum_cantilever_distance: 1.05,
    contact_tolerance: 0.0001,
    anchored_color: [0.08, 0.6, 0.85],
    vertical_color: [0.15, 0.8, 0.35],
    braced_color: [1.0, 0.65, 0.08],
};

/// How the structure is colored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructureView {
    /// Material tints under the warm presentation lights.
    Material,
    /// Remaining resistance, from critical red through solid cyan to reinforced blue.
    Integrity,
    /// Ground connection: anchored, vertically supported, or braced sideways.
    Support,
}

/// The integrity view: remaining resistance in damage units, independent of surface materials.
pub struct IntegrityConfig {
    /// Whether the demo starts in the integrity view.
    pub enabled: bool,
    /// Visual face inset that reveals chunk boundaries without changing colliders.
    pub mesh_inset: f64,
    /// Color at no resistance.
    pub critical_color: [f64; 3],
    /// Color at half the masonry resistance.
    pub weakened_color: [f64; 3],
    /// Color of intact masonry.
    pub intact_color: [f64; 3],
    /// Color of intact frame chunks.
    pub reinforced_color: [f64; 3],
    /// Neutral ambient for the debug views.
    pub ambient: f32,
    /// The debug views' single neutral light.
    pub light: PointLight,
}

/// The example's integrity view.
pub const INTEGRITY: IntegrityConfig = IntegrityConfig {
    enabled: true,
    mesh_inset: 0.985,
    critical_color: [1.0, 0.06, 0.025],
    weakened_color: [1.0, 0.64, 0.08],
    intact_color: [0.08, 0.6, 0.85],
    reinforced_color: [0.2, 0.3, 0.85],
    ambient: 0.7,
    light: PointLight { position: Vec3::new(5.0, 12.0, 9.0), color: [2.0, 2.0, 2.0], range: 45.0, casts_shadow: true },
};

/// World-space impact markers; bounds and lifetime keep repeated clicks inexpensive.
pub struct ImpactDebugConfig {
    /// Whether markers draw at startup.
    pub enabled: bool,
    /// Most markers alive at once; the oldest is replaced.
    pub maximum_markers: usize,
    /// Marker lifetime.
    pub lifetime_seconds: f32,
    /// Fade before expiry.
    pub fade_seconds: f32,
    /// Segments per damage-radius ring.
    pub ring_segments: u32,
    /// Width of every marker line.
    pub line_width: f32,
    /// Length of the hit cross arms.
    pub point_size: f32,
    /// Length of the incoming-ray segment.
    pub ray_length: f32,
    /// Offset back along the ray so markers clear the surface.
    pub surface_offset: f64,
    /// Damage-radius ring color.
    pub radius_color: [f32; 3],
    /// Incoming-ray color.
    pub ray_color: [f32; 3],
    /// Hit cross color.
    pub point_color: [f32; 3],
}

/// The example's impact markers.
pub const IMPACT_DEBUG: ImpactDebugConfig = ImpactDebugConfig {
    enabled: true,
    maximum_markers: 8,
    lifetime_seconds: 6.0,
    fade_seconds: 1.0,
    ring_segments: 48,
    line_width: 0.025,
    point_size: 0.18,
    ray_length: 3.0,
    surface_offset: 0.025,
    radius_color: [0.15, 0.85, 1.0],
    ray_color: [1.0, 0.85, 0.15],
    point_color: [1.0, 1.0, 1.0],
};

/// Camera framing and controls.
pub struct ViewConfig {
    /// Orbit target.
    pub target: Vec3,
    /// Starting eye distance.
    pub distance: f32,
    /// Framed content radius.
    pub radius: f32,
    /// Starting heading.
    pub yaw: f32,
    /// Starting elevation.
    pub pitch: f32,
    /// Near clip.
    pub near_clip: f32,
    /// Far clip.
    pub far_clip: f32,
    /// Closest zoom.
    pub minimum_distance: f32,
    /// Farthest zoom.
    pub maximum_distance: f32,
    /// Lowest elevation.
    pub minimum_pitch: f32,
    /// Highest elevation.
    pub maximum_pitch: f32,
    /// Radians of orbit per logical pixel of drag.
    pub orbit_sensitivity: f32,
    /// Zoom factor exponent per logical pixel of wheel scroll.
    pub zoom_sensitivity: f32,
    /// Pointer travel beyond which a press orbits instead of striking.
    pub drag_threshold_pixels: f32,
}

/// The example's camera.
pub const VIEW: ViewConfig = ViewConfig {
    target: Vec3::new(0.0, 2.1, -0.5),
    distance: 15.0,
    radius: 9.0,
    yaw: 0.05,
    pitch: 0.22,
    near_clip: 0.1,
    far_clip: 300.0,
    minimum_distance: 6.0,
    maximum_distance: 28.0,
    minimum_pitch: 0.03,
    maximum_pitch: 1.25,
    orbit_sensitivity: 0.004,
    zoom_sensitivity: 0.001,
    drag_threshold_pixels: 4.0,
};

/// Material-view lighting, frame bounds, and HUD cadence.
pub struct PresentationConfig {
    /// Linear background color.
    pub clear_color: [f32; 4],
    /// Material-view ambient.
    pub ambient: f32,
    /// Warm key light.
    pub primary_light: PointLight,
    /// Cool fill light behind the wall.
    pub secondary_light: PointLight,
    /// Largest simulated frame step.
    pub maximum_frame_seconds: f32,
    /// Timing samples kept per distribution.
    pub timing_sample_count: usize,
    /// Frames between HUD refreshes.
    pub hud_interval_frames: u64,
}

/// The example's presentation.
pub const PRESENTATION: PresentationConfig = PresentationConfig {
    clear_color: [0.025, 0.035, 0.045, 1.0],
    ambient: 0.42,
    primary_light: PointLight {
        position: Vec3::new(7.0, 14.0, 9.0),
        color: [5.0, 3.2, 1.8],
        range: 45.0,
        casts_shadow: true,
    },
    secondary_light: PointLight {
        position: Vec3::new(-7.0, 5.0, -4.0),
        color: [0.25, 0.7, 1.2],
        range: 24.0,
        casts_shadow: true,
    },
    maximum_frame_seconds: 0.05,
    timing_sample_count: 180,
    hud_interval_frames: 10,
};
