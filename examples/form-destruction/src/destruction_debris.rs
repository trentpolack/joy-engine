// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Persistent rigid debris for released chunks: the ground (a craterable heightfield patch inside flat apron slabs) in
//! the engine's `DebrisWorld` (structure colliders and a budgeted debris pool in one simulation), plus one mesh per
//! debris piece.
//!
//! Directly fractured chunks shatter into Voronoi shards; support failures and overloads leave whole. Surviving chunks
//! keep static colliders, so falling pieces land on what stands, and the pool freezes settled pieces into static rubble
//! once too many are moving. Physics uses meters, seconds, and Y-up world space. Scenes are borrowed per call.

use std::collections::HashMap;

use joy_engine::core::{
    PrimitiveShape,
    SeededRandom,
};
use joy_engine::physics::{
    BodyHandle,
    BodyOptions,
    DebrisOptions,
    DebrisRelease,
    DebrisWorld,
    DebrisWorldOptions,
    DestructibleChunkDefinition,
    PhysicsError,
    PhysicsSimulationOptions,
    StructureColliderOptions,
    create_chunk_mesh_data,
    fracture_chunk,
    fracture_sites,
};
use joy_engine::prelude::*;

use super::config::destruction_config::{
    DESTRUCTION,
    GROUND,
};
use super::ground::CraterGround;

/// How a chunk leaves the structure, which decides its starting size and motion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Release {
    /// Broken by an impact: shattered into shards and launched with spin.
    Fractured,
    /// Lost its support: whole and at rest, with a sliver of clearance so it slides past surviving neighbors.
    Resting,
    /// Crushed under load: whole but shrunk, leaning out of the wall face as it gives way.
    Crumbled,
}

/// Copied motion of one debris body, in meters and meters per second.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code, reason = "part of the capture and test API that the integration tests drive")]
pub struct DebrisPieceSnapshot {
    /// The released piece's chunk id (`<chunk>/<shard>` for shards).
    pub chunk_id: String,
    /// Body position.
    pub position: Vec3,
    /// Body velocity.
    pub velocity: Vec3,
}

/// Copied debris diagnostics.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code, reason = "part of the capture and test API that the integration tests drive")]
pub struct DebrisSnapshot {
    /// World height of the undisturbed ground.
    pub ground_top: f64,
    /// Static colliders matching surviving chunks.
    pub static_chunks: usize,
    /// Every debris body, in release order.
    pub pieces: Vec<DebrisPieceSnapshot>,
}

/// Owns the debris world (with the ground colliders), the ground mesh, and the debris meshes.
pub struct DestructionDebris {
    debris: DebrisWorld,
    ground_mesh: MeshInstanceId,
    // The craterable patch's collider; replaced whenever the ground changes.
    ground_patch: BodyHandle,
    // One mesh per debris piece, keyed like the pool (which keeps the release order).
    meshes: HashMap<String, MeshInstanceId>,
}

impl DestructionDebris {
    /// Create the world and flat, uncratered ground (colliders and mesh, sharing dimensions) in `scene`.
    pub fn new(scene: &mut RenderScene) -> AppResult<Self> {
        let mut debris = DebrisWorld::new(DebrisWorldOptions {
            simulation: PhysicsSimulationOptions {
                gravity: Vec3::new(0.0, -DESTRUCTION.debris_gravity, 0.0),
                fixed_step: DESTRUCTION.physics_step_seconds,
                max_sub_steps: DESTRUCTION.maximum_physics_substeps,
            },
            pool: DebrisOptions {
                friction: DESTRUCTION.debris_friction,
                restitution: DESTRUCTION.debris_restitution,
                linear_damping: DESTRUCTION.debris_linear_damping,
                angular_damping: DESTRUCTION.debris_angular_damping,
                release_scale: DESTRUCTION.released_chunk_scale,
                maximum_moving: DESTRUCTION.maximum_moving_debris,
                maximum_pieces: DESTRUCTION.maximum_debris,
                ..DebrisOptions::default()
            },
            structure: StructureColliderOptions {
                friction: DESTRUCTION.debris_friction,
                restitution: DESTRUCTION.debris_restitution,
                ..StructureColliderOptions::default()
            },
        })?;
        let world = debris.simulation_mut();

        // Four apron slabs frame the patch, with tops at the undisturbed ground height, so craters stay open.
        let apron = GROUND.apron_half_extent;
        let [half_x, half_z] = GROUND.patch_half_extents;
        let slabs = [
            ([0.0, (-apron - half_z) / 2.0], [apron, (apron - half_z) / 2.0]),
            ([0.0, (apron + half_z) / 2.0], [apron, (apron - half_z) / 2.0]),
            ([(-apron - half_x) / 2.0, 0.0], [(apron - half_x) / 2.0, half_z]),
            ([(apron + half_x) / 2.0, 0.0], [(apron - half_x) / 2.0, half_z]),
        ];
        for ([center_x, center_z], [extent_x, extent_z]) in slabs {
            let half_thickness = GROUND.apron_thickness / 2.0;
            world.create_body(BodyOptions {
                position: DVec3::new(center_x, GROUND.height - half_thickness, center_z).as_vec3(),
                half_extents: DVec3::new(extent_x, half_thickness, extent_z).as_vec3(),
                friction: GROUND.friction,
                restitution: GROUND.restitution,
                ..BodyOptions::new(PrimitiveShape::Box)
            })?;
        }

        let ground = CraterGround::new();
        let ground_patch = world.create_heightfield_body(&ground.heightfield())?;
        let ground_mesh = scene.insert(ground_mesh_instance(&ground)?);
        Ok(Self {
            debris,
            ground_mesh,
            ground_patch,
            meshes: HashMap::new(),
        })
    }

    /// Replace the ground's patch collider and mesh with `ground`'s current surface (after a crater or a reset).
    /// Removing the old patch wakes debris resting in it, so it settles onto the new surface on the next steps.
    pub fn set_ground(&mut self, scene: &mut RenderScene, ground: &CraterGround) -> AppResult {
        let world = self.debris.simulation_mut();
        let patch = world.create_heightfield_body(&ground.heightfield())?;
        world.remove_body(std::mem::replace(&mut self.ground_patch, patch));
        let mesh = scene.insert(ground_mesh_instance(ground)?);
        scene.remove(std::mem::replace(&mut self.ground_mesh, mesh));
        Ok(())
    }

    /// Release chunks as debris. Chunks already released are skipped.
    ///
    /// Directly fractured chunks (`apply_impact_impulse`) shatter into Voronoi shards, each a little smaller than its
    /// share so it can leave a tightly fitted wall, launched with an impact velocity and spin. Support failures leave
    /// whole, keeping all but a sliver of clearance, and start at rest.
    pub fn spawn<'a>(
        &mut self,
        scene: &mut RenderScene,
        chunks: impl IntoIterator<Item = &'a DestructibleChunkDefinition>,
        apply_impact_impulse: bool,
    ) -> AppResult {
        let release = if apply_impact_impulse { Release::Fractured } else { Release::Resting };
        self.release(scene, chunks, release)
    }

    /// Release chunks that failed under load: shrunk to the fractured size, leaning out of the wall face (+Z) with a
    /// tip at their top, so an overloaded section reads as giving way rather than vanishing. Chunks already released
    /// are skipped.
    pub fn spawn_crumbled<'a>(
        &mut self,
        scene: &mut RenderScene,
        chunks: impl IntoIterator<Item = &'a DestructibleChunkDefinition>,
    ) -> AppResult {
        self.release(scene, chunks, Release::Crumbled)
    }

    fn release<'a>(
        &mut self,
        scene: &mut RenderScene,
        chunks: impl IntoIterator<Item = &'a DestructibleChunkDefinition>,
        release: Release,
    ) -> AppResult {
        for chunk in chunks {
            let pieces = match release {
                Release::Fractured => shatter(chunk)?,
                Release::Resting | Release::Crumbled => vec![chunk.clone()],
            };
            let chunk_volume = chunk.volume();
            for piece in &pieces {
                // An impact's spin divides among the shards by their share of the chunk, so small shards do not spin
                // faster than the whole block would.
                let share = (piece.volume() / chunk_volume) as f32;
                self.release_piece(scene, piece, release, share)?;
            }
        }
        Ok(())
    }

    /// Release one piece through the pool, give it its motion (impulses scaled by `share` of its chunk), and add its
    /// mesh.
    fn release_piece(
        &mut self,
        scene: &mut RenderScene,
        piece: &DestructibleChunkDefinition,
        release: Release,
        share: f32,
    ) -> AppResult {
        let scale = match release {
            Release::Fractured | Release::Crumbled => DESTRUCTION.fractured_chunk_scale,
            Release::Resting => DESTRUCTION.released_chunk_scale,
        };
        let velocity = match release {
            Release::Fractured => {
                // Pieces fly away from the wall's middle, varied by release order.
                let direction = if piece.center[0] < 0.0 { -1.0 } else { 1.0 };
                let count = self.debris.len();
                let [launch_x, launch_y, launch_z] = DESTRUCTION.debris_launch_speed;
                let [vary_x, vary_y] = DESTRUCTION.debris_launch_variation;
                Vec3::new(
                    direction * (launch_x + ((count % 3) as f32 * vary_x)),
                    launch_y + ((count % 4) as f32 * vary_y),
                    launch_z,
                )
            }
            Release::Crumbled => Vec3::new(0.0, 0.0, DESTRUCTION.crumble_lean_speed),
            Release::Resting => Vec3::ZERO,
        };
        let request = DebrisRelease {
            velocity,
            mass: Some(DESTRUCTION.debris_mass * share),
            scale: Some(scale),
            rotation: Vec3::ZERO,
        };
        let Some(body) = self.debris.release(piece, request, None)? else {
            return Ok(());
        };
        let top = piece.center.as_vec3() + Vec3::new(0.0, (piece.size[1] / 2.0) as f32, 0.0);
        match release {
            Release::Fractured => {
                let spin = Vec3::from_array(DESTRUCTION.debris_spin_impulse) * share;
                self.debris.apply_impulse(body, spin, Some(top))?;
            }
            Release::Crumbled => {
                self.debris.apply_impulse(body, Vec3::from_array(DESTRUCTION.crumble_tip_impulse), Some(top))?;
            }
            Release::Resting => {}
        }

        let Some(released) = self.debris.piece(&piece.id) else {
            return Ok(());
        };
        let asset = MeshAsset::new(&create_chunk_mesh_data([&released.local_chunk()]))?;
        let start = self.debris.simulation().body_transform(body).map_or(piece.center.as_vec3(), |pose| pose.position);
        let mesh = MeshInstance::new(asset).with_transform(Transform::new(start, Vec3::ZERO, Vec3::ONE));
        self.meshes.insert(piece.id.clone(), scene.insert(mesh));
        Ok(())
    }

    /// Match static colliders to the committed structure before releasing chunks into rigid-body simulation, so
    /// surviving walls catch falling debris and removed colliders never overlap their new dynamic bodies.
    pub fn synchronize_structure<'a>(
        &mut self,
        chunks: impl IntoIterator<Item = &'a DestructibleChunkDefinition>,
    ) -> Result<(), PhysicsError> {
        self.debris.sync_structure(0, chunks)
    }

    /// Advance bounded fixed-step physics by `elapsed_seconds`, keep the pool within its budget, and draw every piece
    /// at its interpolated pose.
    pub fn update(&mut self, scene: &mut RenderScene, elapsed_seconds: f32) -> Result<(), PhysicsError> {
        let step = self.debris.step(f64::from(elapsed_seconds));
        for id in &step.removed {
            if let Some(mesh) = self.meshes.remove(id) {
                scene.remove(mesh);
            }
        }
        for piece in self.debris.pieces() {
            let (Some(pose), Some(mesh)) = (
                self.debris.interpolated_pose(&piece.chunk_id),
                self.meshes.get(&piece.chunk_id).and_then(|mesh| scene.get_mut(*mesh)),
            ) else {
                continue;
            };
            mesh.transform.position = pose.position;
            mesh.transform.rotation = pose.rotation;
        }
        step.failure.map_or(Ok(()), Err)
    }

    /// Copied diagnostics.
    #[allow(dead_code, reason = "part of the capture and test API that the integration tests drive")]
    pub fn snapshot(&self) -> DebrisSnapshot {
        DebrisSnapshot {
            ground_top: GROUND.height,
            static_chunks: self.debris.structure_collider_count(),
            pieces: self
                .debris
                .pieces()
                .map(|piece| DebrisPieceSnapshot {
                    chunk_id: piece.chunk_id.clone(),
                    position: self.debris.pose(&piece.chunk_id).map_or(Vec3::ZERO, |pose| pose.position),
                    velocity: self.debris.velocity(&piece.chunk_id).unwrap_or(Vec3::ZERO),
                })
                .collect(),
        }
    }

    /// Debris pieces retained.
    pub fn count(&self) -> usize {
        self.debris.len()
    }

    /// Release every debris body and mesh, keeping the ground for the next reset.
    pub fn reset(&mut self, scene: &mut RenderScene) {
        self.debris.clear_pieces();
        for (_, mesh) in self.meshes.drain() {
            scene.remove(mesh);
        }
    }

    /// Release the ground mesh and every debris mesh from `scene`; the world and its bodies drop with `self`.
    #[allow(dead_code, reason = "part of the capture and test API that the integration tests drive")]
    pub fn dispose(mut self, scene: &mut RenderScene) {
        self.reset(scene);
        scene.remove(self.ground_mesh);
    }
}

/// A chunk broken into its Voronoi shards, seeded by its id so the same chunk always shatters the same way.
fn shatter(chunk: &DestructibleChunkDefinition) -> AppResult<Vec<DestructibleChunkDefinition>> {
    let mut random = SeededRandom::new(DESTRUCTION.seed ^ id_seed(&chunk.id));
    let sites = fracture_sites(chunk.center, chunk.size, DESTRUCTION.shards_per_fracture, &mut random);
    Ok(fracture_chunk(chunk, &sites)?)
}

/// A stable seed from a chunk id (FNV-1a), independent of release order and platform.
fn id_seed(id: &str) -> i64 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in id.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    i64::from(hash)
}

/// The ground's mesh, in its own neutral tint, independent of the chunk material palette.
fn ground_mesh_instance(ground: &CraterGround) -> AppResult<MeshInstance> {
    Ok(MeshInstance::new(MeshAsset::new(&ground.mesh_data())?).with_tint(GROUND.tint))
}
