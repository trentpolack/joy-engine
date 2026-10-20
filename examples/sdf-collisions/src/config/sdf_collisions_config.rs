// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Named tuning for the SDF collisions example: arena layout, the shared spawn sequence, physics materials, the bake
//! resolution, the benchmark scenario, the camera, and presentation. Lengths are meters, times seconds, masses
//! kilograms, and angles radians.

use joy_engine::physics::SdfBakeOptions;
use joy_engine::prelude::*;

/// Two identical bins side by side: SDF colliders on the left, the props' bounding boxes on the right.
pub struct ArenaSettings {
    /// Distance between the two bins' centers along X.
    pub separation: f32,
    /// Half the inner width of a bin along X and Z.
    pub inner_half_extent: f32,
    /// Wall height above the bin floor.
    pub wall_height: f32,
    /// Wall and floor slab thickness.
    pub wall_thickness: f32,
    /// Bodies farther than this below the floor, or outside the bin by this much, count as escaped (tunneled).
    pub escape_margin: f32,
}

/// The spawn sequence. Both bins draw it from identically seeded streams, so they receive the same props in the same
/// poses at the same times.
pub struct SpawnSettings {
    /// Seed of both bins' spawn streams.
    pub seed: i64,
    /// Seconds between drops.
    pub interval_seconds: f32,
    /// Drop height above the bin floor.
    pub height: f32,
    /// Half the width of the square drop area, centered on the bin.
    pub spread: f32,
    /// Props per bin before the pile is complete.
    pub initial_count: usize,
    /// Props added by DROP MORE.
    pub burst: usize,
    /// Most props a bin holds.
    pub maximum_count: usize,
}

/// Rigid-body materials and stepping.
pub struct PhysicsSettings {
    /// Fixed simulation step in seconds (the engine default).
    pub fixed_step: f32,
    /// Most fixed steps per frame.
    pub max_sub_steps: u32,
    /// Prop density in kilograms per cubic meter (about pine); mass is the SDF volume times this in both bins.
    pub density: f32,
    /// Friction of props, floors, and walls.
    pub friction: f32,
    /// Restitution of props.
    pub restitution: f32,
    /// Per-second linear and angular velocity loss fractions (engine semantics).
    pub linear_damping: f32,
    pub angular_damping: f32,
    /// Distance-grid resolution for meshes baked from FORM.
    pub bake: SdfBakeOptions,
}

/// The headless benchmark behind `--bench` (native) and `?bench=1` (browsers).
pub struct BenchmarkSettings {
    /// Launch parameter that runs it.
    pub parameter: &'static str,
    /// Fixed steps simulated per bin.
    pub steps: u32,
    /// Props dropped into each bin.
    pub props: usize,
}

/// Orbit camera framing and controls.
pub struct ViewSettings {
    /// Orbit center, between the two bins.
    pub target: Vec3,
    /// Starting distance from the target.
    pub distance: f32,
    /// Starting heading and elevation.
    pub yaw: f32,
    pub pitch: f32,
    /// Near and far clip distances.
    pub clips: (f32, f32),
    /// Elevation and zoom limits.
    pub pitch_limits: (f32, f32),
    pub distance_limits: (f32, f32),
    /// Radians of orbit per logical pixel dragged, and the exponential zoom rate per scroll unit.
    pub orbit_sensitivity: f32,
    pub zoom_sensitivity: f32,
}

/// Lights, colors, and timing.
pub struct PresentationSettings {
    /// Ambient light strength and the background color.
    pub ambient: f32,
    pub clear_color: LinearColor,
    /// A warm shadowed key light above the front-left, and a cool fill from behind.
    pub key_light: PointLight,
    pub fill_light: PointLight,
    /// Bin floor tints: SDF bin, then box bin. The HUD legend uses the same colors.
    pub sdf_tint: LinearColor,
    pub box_tint: LinearColor,
    pub wall_tint: LinearColor,
    /// Timing samples kept per distribution.
    pub timing_samples: usize,
    /// Longest frame the simulation advances, in seconds; hidden tabs and stalls never fast-forward the piles.
    pub maximum_frame_seconds: f32,
}

pub const ARENA: ArenaSettings = ArenaSettings {
    separation: 6.2,
    inner_half_extent: 1.9,
    wall_height: 1.0,
    wall_thickness: 0.2,
    escape_margin: 1.0,
};

pub const SPAWN: SpawnSettings = SpawnSettings {
    seed: 2026,
    interval_seconds: 0.3,
    height: 4.2,
    spread: 1.2,
    initial_count: 30,
    burst: 10,
    maximum_count: 90,
};

pub const PHYSICS: PhysicsSettings = PhysicsSettings {
    fixed_step: 1.0 / 60.0,
    max_sub_steps: 4,
    density: 450.0,
    friction: 0.6,
    restitution: 0.05,
    linear_damping: 0.05,
    angular_damping: 0.1,
    bake: SdfBakeOptions {
        resolution: 40,
    },
};

pub const BENCHMARK: BenchmarkSettings = BenchmarkSettings {
    parameter: "bench",
    steps: 1200,
    props: 50,
};

pub const VIEW: ViewSettings = ViewSettings {
    target: Vec3::new(0.0, 0.6, 0.0),
    distance: 12.5,
    yaw: 0.0,
    pitch: 0.8,
    clips: (0.1, 200.0),
    pitch_limits: (0.05, 1.45),
    distance_limits: (3.0, 60.0),
    orbit_sensitivity: 0.005,
    zoom_sensitivity: 0.001,
};

pub const PRESENTATION: PresentationSettings = PresentationSettings {
    ambient: 0.75,
    clear_color: LinearColor::new(0.055, 0.075, 0.095, 1.0),
    key_light: PointLight {
        position: Vec3::new(-4.0, 11.0, 7.0),
        color: LinearColor::rgb(8.0, 7.4, 6.6),
        range: 40.0,
        casts_shadow: true,
    },
    fill_light: PointLight {
        position: Vec3::new(6.0, 7.0, -6.0),
        color: LinearColor::rgb(2.0, 2.4, 3.0),
        range: 30.0,
        casts_shadow: false,
    },
    sdf_tint: LinearColor::rgb(0.27, 0.62, 0.58),
    box_tint: LinearColor::rgb(0.74, 0.42, 0.27),
    wall_tint: LinearColor::rgb(0.32, 0.35, 0.38),
    timing_samples: 120,
    maximum_frame_seconds: 0.1,
};
