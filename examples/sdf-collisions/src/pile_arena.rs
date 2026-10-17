// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! One bin and its growing pile: an independent physics world with a static floor and walls, props dropped on a
//! seeded schedule, and (when a scene is supplied) one mesh per body that follows it.
//!
//! The SDF bin gives each prop its signed distance collider. The box bin gives each prop its bounding box, which is
//! what the engine could offer before SDF colliders; boxes are centered on the bounds, so their meshes are offset to
//! match. Both bins draw the same seeded spawn stream and step the same fixed time, so their piles differ only in
//! collision. Worlds use meters, seconds, and Y-up space. Scenes are borrowed per call; `None` runs headless.

use joy_engine::core::PrimitiveShape;
use joy_engine::core::js_math;
use joy_engine::physics::{
    BodyHandle,
    BodyOptions,
    PhysicsError,
    SdfCollisionStatistics,
};
use joy_engine::prelude::*;
use joy_engine::runtime::Stopwatch;

use super::config::sdf_collisions_config::{
    ARENA,
    PHYSICS,
    PRESENTATION,
    SPAWN,
};
use super::prop_library::PropLibrary;

/// Which collider each prop receives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColliderMode {
    /// The prop's signed distance field.
    Sdf,
    /// The prop's local bounding box.
    BoundingBox,
}

/// One dropped prop: its body, its prop index, and its mesh when a scene is attached.
struct Piece {
    body: BodyHandle,
    prop: usize,
    mesh: Option<MeshInstanceId>,
}

/// Copied bin diagnostics; timings are CPU milliseconds per fixed step.
#[derive(Clone, Debug, PartialEq)]
pub struct ArenaSnapshot {
    /// Props dropped so far.
    pub bodies: usize,
    /// Props the pile grows to.
    pub target: usize,
    /// Bodies that left the bin (fell through the floor or out over the walls).
    pub escaped: usize,
    /// Mean and worst recent physics time per fixed step.
    pub mean_step_milliseconds: f64,
    pub worst_step_milliseconds: f64,
    /// SDF contact work of the latest fixed step (all zero in the box bin).
    pub statistics: SdfCollisionStatistics,
}

/// A bin, its world, and its pile.
pub struct PileArena {
    // Configuration.
    mode: ColliderMode,
    center_x: f32,

    // Simulation. Pieces are in drop order, which is also the order the hash and meshes visit them.
    world: PhysicsWorld,
    random: SeededRandom,
    pieces: Vec<Piece>,
    target: usize,
    spawn_clock: f32,

    // Presentation; empty when headless.
    bin_meshes: Vec<MeshInstanceId>,

    // CPU timing samples in milliseconds per fixed step.
    step_samples: Vec<f64>,
}

impl PileArena {
    /// An empty bin centered at `center_x`, with its floor top at y = 0. Adds the bin's meshes to `scene` if given.
    pub fn new(mode: ColliderMode, center_x: f32, scene: Option<&mut RenderScene>) -> Result<Self, PhysicsError> {
        let mut world = PhysicsWorld::new(PhysicsWorldOptions {
            fixed_step: PHYSICS.fixed_step,
            max_sub_steps: PHYSICS.max_sub_steps,
            ..PhysicsWorldOptions::default()
        })?;
        let slabs = bin_slabs(center_x);
        for (center, half_extents) in &slabs {
            world.create_body(BodyOptions {
                position: *center,
                half_extents: *half_extents,
                friction: PHYSICS.friction,
                ..BodyOptions::new(PrimitiveShape::Box)
            })?;
        }

        let mut bin_meshes = Vec::new();
        if let Some(scene) = scene {
            let floor_tint = if mode == ColliderMode::Sdf { PRESENTATION.sdf_tint } else { PRESENTATION.box_tint };
            for (index, (center, half_extents)) in slabs.iter().enumerate() {
                let tint = if index == 0 { floor_tint } else { PRESENTATION.wall_tint };
                let transform = Transform::new(*center, Vec3::ZERO, *half_extents * 2.0);
                bin_meshes
                    .push(scene.insert(MeshInstance::new(primitive_box()).with_transform(transform).with_tint(tint)));
            }
        }

        Ok(Self {
            mode,
            center_x,
            world,
            random: SeededRandom::new(SPAWN.seed),
            pieces: Vec::new(),
            target: SPAWN.initial_count,
            spawn_clock: 0.0,
            bin_meshes,
            step_samples: Vec::new(),
        })
    }

    /// Drop props that are due, advance physics by `elapsed_seconds` in fixed steps, and move meshes to their bodies.
    pub fn update(
        &mut self,
        elapsed_seconds: f32,
        library: &PropLibrary,
        mut scene: Option<&mut RenderScene>,
    ) -> Result<(), PhysicsError> {
        self.spawn_clock += elapsed_seconds;
        while self.spawn_clock >= SPAWN.interval_seconds && self.pieces.len() < self.target {
            self.spawn_clock -= SPAWN.interval_seconds;
            self.spawn(library, scene.as_deref_mut())?;
        }
        if self.pieces.len() >= self.target {
            self.spawn_clock = 0.0;
        }

        let stopwatch = Stopwatch::start();
        let steps = self.world.step(elapsed_seconds)?;
        if steps > 0 {
            retain(&mut self.step_samples, stopwatch.elapsed_milliseconds() / f64::from(steps));
        }

        if let Some(scene) = scene {
            for piece in &self.pieces {
                let (Some(pose), Some(mesh)) = (self.world.body_transform(piece.body), piece.mesh) else {
                    continue;
                };
                if let Some(instance) = scene.get_mut(mesh) {
                    instance.transform.position = pose.position - self.mesh_offset(library, piece.prop, pose.rotation);
                    instance.transform.rotation = pose.rotation;
                }
            }
        }
        Ok(())
    }

    /// Grow the pile by `count` props, up to the configured maximum.
    pub fn raise_target(&mut self, count: usize) {
        self.set_target(self.target + count);
    }

    /// Set how many props the pile grows to, up to the configured maximum; props already dropped stay.
    pub fn set_target(&mut self, count: usize) {
        self.target = count.min(SPAWN.maximum_count);
    }

    /// Remove every prop and its mesh, restart the spawn stream, and restore the initial pile size.
    pub fn reset(&mut self, mut scene: Option<&mut RenderScene>) {
        for piece in self.pieces.drain(..) {
            self.world.remove_body(piece.body);
            if let (Some(scene), Some(mesh)) = (scene.as_deref_mut(), piece.mesh) {
                scene.remove(mesh);
            }
        }
        self.random = SeededRandom::new(SPAWN.seed);
        self.target = SPAWN.initial_count;
        self.spawn_clock = 0.0;
        self.step_samples.clear();
    }

    /// Copied diagnostics.
    pub fn snapshot(&self) -> ArenaSnapshot {
        let worst = self.step_samples.iter().copied().fold(0.0, f64::max);
        ArenaSnapshot {
            bodies: self.pieces.len(),
            target: self.target,
            escaped: self.escaped(),
            mean_step_milliseconds: mean(&self.step_samples),
            worst_step_milliseconds: worst,
            statistics: self.world.sdf_collision_statistics(),
        }
    }

    /// FNV-1a hash of every body's position bits in drop order; equal hashes mean identical piles.
    ///
    /// Positions come straight from the solver. Rotations are left out: their Euler decomposition uses platform math,
    /// and a pile whose rotations diverged would not keep identical positions anyway.
    pub fn position_hash(&self) -> u64 {
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut hash = OFFSET;
        for piece in &self.pieces {
            let Some(pose) = self.world.body_transform(piece.body) else {
                continue;
            };
            for value in pose.position.to_array() {
                for byte in value.to_bits().to_le_bytes() {
                    hash = (hash ^ u64::from(byte)).wrapping_mul(PRIME);
                }
            }
        }
        hash
    }

    /// Release the bin's meshes and every prop mesh from `scene`; the world drops with `self`.
    #[allow(dead_code, reason = "part of the test API that test/rust drives")]
    pub fn dispose(mut self, scene: &mut RenderScene) {
        self.reset(Some(scene));
        for mesh in self.bin_meshes.drain(..) {
            scene.remove(mesh);
        }
    }

    /// Drop the next prop of the seeded stream above the bin.
    fn spawn(&mut self, library: &PropLibrary, scene: Option<&mut RenderScene>) -> Result<(), PhysicsError> {
        // Every drop draws the same six values in the same order, so both bins stay in lockstep.
        let prop_index = ((unit(&mut self.random) * library.len() as f32) as usize).min(library.len() - 1);
        let offset_x = (unit(&mut self.random) * 2.0 - 1.0) * SPAWN.spread;
        let offset_z = (unit(&mut self.random) * 2.0 - 1.0) * SPAWN.spread;
        let rotation = Vec3::new(
            unit(&mut self.random) * std::f32::consts::TAU,
            unit(&mut self.random) * std::f32::consts::TAU,
            unit(&mut self.random) * std::f32::consts::TAU,
        );
        let prop = library.prop(prop_index);
        let origin = Vec3::new(self.center_x + offset_x, SPAWN.height, offset_z);

        let options = BodyOptions {
            mass: prop.mass,
            rotation,
            friction: PHYSICS.friction,
            restitution: PHYSICS.restitution,
            linear_damping: PHYSICS.linear_damping,
            angular_damping: PHYSICS.angular_damping,
            ..BodyOptions::default()
        };
        let body = match self.mode {
            ColliderMode::Sdf => {
                self.world.create_sdf_body(&prop.shape, BodyOptions { position: origin, ..options })?
            }
            ColliderMode::BoundingBox => {
                let position = origin + rotate_portably(prop.box_center, rotation);
                let half_extents = prop.box_half_extents;
                self.world.create_body(BodyOptions { position, half_extents, ..options })?
            }
        };

        let mesh = scene.map(|scene| {
            let transform = Transform::new(origin, rotation, Vec3::ONE);
            scene.insert(MeshInstance::new(prop.asset.clone()).with_transform(transform).with_shading(Shading::Pbr))
        });
        self.pieces.push(Piece { body, prop: prop_index, mesh });
        Ok(())
    }

    /// World offset from a body's origin to its mesh's origin: none for SDF bodies, the rotated box center for boxes.
    fn mesh_offset(&self, library: &PropLibrary, prop: usize, rotation: Vec3) -> Vec3 {
        match self.mode {
            ColliderMode::Sdf => Vec3::ZERO,
            ColliderMode::BoundingBox => rotate_portably(library.prop(prop).box_center, rotation),
        }
    }

    /// Bodies below the floor or outside the walls by more than the escape margin.
    fn escaped(&self) -> usize {
        let limit = ARENA.inner_half_extent + ARENA.wall_thickness + ARENA.escape_margin;
        self.pieces
            .iter()
            .filter_map(|piece| self.world.body_transform(piece.body))
            .filter(|pose| {
                let outside = (pose.position.x - self.center_x).abs() > limit || pose.position.z.abs() > limit;
                pose.position.y < -ARENA.escape_margin || outside
            })
            .count()
    }
}

/// The bin's floor slab followed by its four walls, as (center, half extents) in world meters.
fn bin_slabs(center_x: f32) -> [(Vec3, Vec3); 5] {
    let inner = ARENA.inner_half_extent;
    let thickness = ARENA.wall_thickness;
    let outer = inner + thickness;
    let wall_y = ARENA.wall_height * 0.5;
    let half_height = ARENA.wall_height * 0.5;
    [
        (Vec3::new(center_x, -thickness * 0.5, 0.0), Vec3::new(outer, thickness * 0.5, outer)),
        (Vec3::new(center_x - inner - (thickness * 0.5), wall_y, 0.0), Vec3::new(thickness * 0.5, half_height, outer)),
        (Vec3::new(center_x + inner + (thickness * 0.5), wall_y, 0.0), Vec3::new(thickness * 0.5, half_height, outer)),
        (Vec3::new(center_x, wall_y, -inner - (thickness * 0.5)), Vec3::new(inner, half_height, thickness * 0.5)),
        (Vec3::new(center_x, wall_y, inner + (thickness * 0.5)), Vec3::new(inner, half_height, thickness * 0.5)),
    ]
}

/// Rotate `offset` by XYZ Euler radians applied X, then Y, then Z, with portable trigonometry, so box bodies start at
/// the same place on every target (the engine's `quat_from_euler` uses platform math, which is fine for drawing).
fn rotate_portably(offset: Vec3, rotation: Vec3) -> Vec3 {
    let [x, y, z] = offset.to_array().map(f64::from);
    let [(sine_x, cosine_x), (sine_y, cosine_y), (sine_z, cosine_z)] =
        rotation.to_array().map(|angle| (js_math::sin(f64::from(angle)), js_math::cos(f64::from(angle))));
    let (y1, z1) = ((y * cosine_x) - (z * sine_x), (y * sine_x) + (z * cosine_x));
    let (x2, z2) = ((x * cosine_y) + (z1 * sine_y), (z1 * cosine_y) - (x * sine_y));
    let (x3, y3) = ((x2 * cosine_z) - (y1 * sine_z), (x2 * sine_z) + (y1 * cosine_z));
    Vec3::new(x3 as f32, y3 as f32, z2 as f32)
}

/// Next value of the spawn stream in `[0, 1)`.
fn unit(random: &mut SeededRandom) -> f32 {
    random.next_unit() as f32
}

/// Keep the newest samples of a bounded timing distribution.
fn retain(samples: &mut Vec<f64>, value: f64) {
    samples.push(value);
    if samples.len() > PRESENTATION.timing_samples {
        samples.remove(0);
    }
}

/// Mean of a timing distribution, or zero without samples.
fn mean(samples: &[f64]) -> f64 {
    if samples.is_empty() { 0.0 } else { samples.iter().sum::<f64>() / samples.len() as f64 }
}
