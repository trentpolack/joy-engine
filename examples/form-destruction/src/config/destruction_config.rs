// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM destruction tuning. Dimensions are meters, gravity meters per second squared, launch velocities meters per
//! second, impulses newton-seconds, masses kilograms, densities kilograms per cubic meter, strengths kilograms per
//! square meter of contact, angles radians, times seconds, and drag distances logical pixels. Colors are linear RGB.

pub use joy_engine::physics::destruction::{
    FRAME_MATERIAL,
    INTERIOR_MATERIAL,
};
use joy_engine::physics::{
    ChunkMaterialStrength,
    ChunkStressOptions,
    ChunkSupportOptions,
};
use joy_engine::prelude::*;

/// Material name of ordinary wall chunks; interior chunks are weaker and the return-shell frame is stronger.
pub const MASONRY_MATERIAL: &str = "masonry";

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
    /// Voronoi shards a directly fractured chunk breaks into.
    pub shards_per_fracture: usize,
    /// Debris pieces that keep simulating before the oldest sleeping ones freeze into static rubble.
    pub maximum_moving_debris: usize,
    /// Debris pieces kept at all; the oldest leave beyond it.
    pub maximum_debris: usize,
    /// Size fraction a support-released or overloaded chunk keeps: just enough clearance to slide past its neighbors.
    pub released_chunk_scale: f64,
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
    /// Speed out of the wall face (+Z) of a chunk that fails under load.
    pub crumble_lean_speed: f32,
    /// Tipping impulse applied at the top of a chunk that fails under load.
    pub crumble_tip_impulse: [f32; 3],
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
    shards_per_fracture: 4,
    maximum_moving_debris: 192,
    maximum_debris: 2048,
    released_chunk_scale: 0.97,
    debris_friction: 0.7,
    debris_restitution: 0.08,
    debris_linear_damping: 0.12,
    debris_angular_damping: 0.25,
    debris_launch_speed: [0.8, 0.25, 1.8],
    debris_launch_variation: [0.35, 0.08],
    debris_spin_impulse: [0.2, 0.0, 0.25],
    crumble_lean_speed: 0.9,
    crumble_tip_impulse: [0.0, 0.0, 0.35],
    physics_step_seconds: 1.0 / 120.0,
    maximum_physics_substeps: 6,
};

/// The ground: a craterable heightfield patch around the structure inside a flat outer apron. The rendered ground and
/// its colliders share these values; the ground's undisturbed top is at world Y = 0.
pub struct GroundConfig {
    /// Undisturbed ground height.
    pub height: f64,
    /// Half extents of the craterable patch along X and Z, centered on the origin.
    pub patch_half_extents: [f64; 2],
    /// Distance between heightfield samples in the patch.
    pub sample_spacing: f64,
    /// Half extent of the flat apron along X and Z.
    pub apron_half_extent: f64,
    /// Thickness of the apron's collision slabs below the ground.
    pub apron_thickness: f64,
    /// Neutral ground tint, independent of the chunk material palette.
    pub tint: LinearColor,
    /// Ground friction.
    pub friction: f32,
    /// Ground restitution.
    pub restitution: f32,
}

/// The example's ground.
pub const GROUND: GroundConfig = GroundConfig {
    height: 0.0,
    patch_half_extents: [14.0, 10.0],
    sample_spacing: 0.25,
    apron_half_extent: 100.0,
    apron_thickness: 2.0,
    tint: LinearColor::rgb(0.3, 0.36, 0.42),
    friction: 0.8,
    restitution: 0.08,
};

/// The CHARGE tool: a ground blast that craters the terrain under the aimed point, undermining whatever stood on it.
pub struct ChargeConfig {
    /// Crater bowl radius.
    pub radius: f64,
    /// How far the bowl's center sinks.
    pub depth: f64,
    /// How far the lip rises at the bowl's edge.
    pub rim_height: f64,
    /// How far below a chunk's bottom the ground may sit while still bearing it; covers sampling error.
    pub bearing_tolerance: f64,
}

/// The example's charge.
pub const CHARGE: ChargeConfig = ChargeConfig {
    radius: 1.6,
    depth: 1.2,
    rim_height: 0.12,
    bearing_tolerance: 0.05,
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
    ground_height: GROUND.height,
    maximum_cantilever_distance: 1.05,
    contact_tolerance: 0.0001,
    anchored_color: [0.08, 0.6, 0.85],
    vertical_color: [0.15, 0.8, 0.35],
    braced_color: [1.0, 0.65, 0.08],
};

/// Load paths: each chunk carries its own mass plus everything resting on it; weakened or undermined chunks overload
/// and fail after a telegraphed delay. Intact masonry bases sit near a third of their capacity.
pub struct StressConfig {
    /// Masonry density and bearing strength.
    pub masonry: (f64, f64),
    /// Interior density and bearing strength: lighter and weaker than masonry.
    pub interior: (f64, f64),
    /// Frame density and bearing strength: heavier and much stronger.
    pub frame: (f64, f64),
    /// Fraction of a material's strength that side faces carry when a chunk hangs from its neighbors.
    pub lateral_strength_fraction: f64,
    /// Load ratio at which a chunk reads as strained.
    pub warning_ratio: f64,
    /// Seconds an overloaded chunk holds before failing.
    pub failure_delay_seconds: f64,
    /// STRESS view color of lightly loaded chunks.
    pub stable_color: [f64; 3],
    /// STRESS view color at the warning ratio.
    pub strained_color: [f64; 3],
    /// Color of overloaded chunks in every view but SUPPORT, pulsing toward white.
    pub failing_color: [f64; 3],
    /// How strongly material and integrity views tint strained chunks toward the strained color, 0 to 1.
    pub strained_tint: f64,
    /// Failing chunks' pulse rate in cycles per second.
    pub pulse_hertz: f64,
    /// Largest sideways shake of a failing chunk just before it fails.
    pub shake_amplitude: f64,
}

impl StressConfig {
    /// The engine stress rule these values configure.
    pub fn options(&self) -> ChunkStressOptions {
        let material = |name: &str, (density, strength): (f64, f64)| ChunkMaterialStrength {
            material: name.to_owned(),
            density,
            strength,
        };
        ChunkStressOptions {
            materials: vec![
                material(MASONRY_MATERIAL, self.masonry),
                material(INTERIOR_MATERIAL, self.interior),
                material(FRAME_MATERIAL, self.frame),
            ],
            default_density: self.masonry.0,
            default_strength: self.masonry.1,
            lateral_strength_fraction: self.lateral_strength_fraction,
            warning_ratio: self.warning_ratio,
            failure_delay_seconds: self.failure_delay_seconds,
        }
    }
}

/// The example's stress rule.
pub const STRESS: StressConfig = StressConfig {
    masonry: (2000.0, 34000.0),
    interior: (1600.0, 24000.0),
    frame: (2400.0, 60000.0),
    lateral_strength_fraction: 0.25,
    warning_ratio: 0.7,
    failure_delay_seconds: 1.5,
    stable_color: [0.12, 0.62, 0.42],
    strained_color: [1.0, 0.72, 0.1],
    failing_color: [1.0, 0.08, 0.03],
    strained_tint: 0.35,
    pulse_hertz: 3.0,
    shake_amplitude: 0.035,
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
    /// Load over capacity: stable green, strained amber, failing red.
    Stress,
}

/// What a click does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    /// Radial damage at the aimed chunk.
    #[default]
    Strike,
    /// A crater in the ground under the aimed point.
    Charge,
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
    light: PointLight {
        position: Vec3::new(5.0, 12.0, 9.0),
        color: LinearColor::rgb(2.0, 2.0, 2.0),
        range: 45.0,
        casts_shadow: true,
    },
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
    pub clear_color: LinearColor,
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
    clear_color: LinearColor::new(0.025, 0.035, 0.045, 1.0),
    ambient: 0.42,
    primary_light: PointLight {
        position: Vec3::new(7.0, 14.0, 9.0),
        color: LinearColor::rgb(5.0, 3.2, 1.8),
        range: 45.0,
        casts_shadow: true,
    },
    secondary_light: PointLight {
        position: Vec3::new(-7.0, 5.0, -4.0),
        color: LinearColor::rgb(0.25, 0.7, 1.2),
        range: 24.0,
        casts_shadow: true,
    },
    maximum_frame_seconds: 0.05,
    timing_sample_count: 180,
    hud_interval_frames: 10,
};
