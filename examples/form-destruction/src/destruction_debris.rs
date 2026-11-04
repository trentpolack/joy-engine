// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Persistent rigid debris for released chunks: the ground (a craterable heightfield patch inside flat apron slabs),
//! one engine physics body and mesh per released chunk, and static colliders that keep the surviving structure solid.
//!
//! Physics uses meters, seconds, and Y-up world space. Debris persists until reset: no expiry or eviction, and one body
//! per chunk id bounds the example to its 126 generated chunks. Scenes are borrowed per call, never held.

use joy_engine::core::PrimitiveShape;
use joy_engine::physics::{
    BodyHandle,
    BodyOptions,
    DestructibleChunkDefinition,
    PhysicsError,
    PhysicsWorld,
    PhysicsWorldOptions,
    create_chunk_mesh_data,
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
    /// Broken by an impact: shrunk slightly and launched with spin.
    Fractured,
    /// Lost its support: at rest, with a sliver of clearance so it slides past surviving neighbors.
    Resting,
    /// Crushed under load: shrunk like a fracture, leaning out of the wall face as it gives way.
    Crumbled,
}

/// One released chunk: its dynamic body and the mesh that follows it.
struct DebrisPiece {
    body: BodyHandle,
    mesh: MeshInstanceId,
}

/// Copied motion of one debris body, in meters and meters per second.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code, reason = "part of the capture and test API that the integration tests drive")]
pub struct DebrisPieceSnapshot {
    /// The released chunk's id.
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

/// Owns the physics world, the ground mesh and colliders, debris, and static chunk colliders.
pub struct DestructionDebris {
    world: PhysicsWorld,
    ground_mesh: MeshInstanceId,
    // The craterable patch's collider; replaced whenever the ground changes.
    ground_patch: BodyHandle,
    // Released chunks and surviving-chunk colliders by chunk id, in insertion order (at most one per chunk).
    pieces: Vec<(String, DebrisPiece)>,
    static_chunks: Vec<(String, BodyHandle)>,
}

impl DestructionDebris {
    /// Create the world and flat, uncratered ground (colliders and mesh, sharing dimensions) in `scene`.
    pub fn new(scene: &mut RenderScene) -> AppResult<Self> {
        let mut world = PhysicsWorld::new(PhysicsWorldOptions {
            gravity: Vec3::new(0.0, -DESTRUCTION.debris_gravity, 0.0),
            fixed_step: DESTRUCTION.physics_step_seconds,
            max_sub_steps: DESTRUCTION.maximum_physics_substeps,
        })?;

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
            world,
            ground_mesh,
            ground_patch,
            pieces: Vec::new(),
            static_chunks: Vec::new(),
        })
    }

    /// Replace the ground's patch collider and mesh with `ground`'s current surface (after a crater or a reset).
    /// Debris resting in the patch settles onto the new surface on the next steps.
    pub fn set_ground(&mut self, scene: &mut RenderScene, ground: &CraterGround) -> AppResult {
        let patch = self.world.create_heightfield_body(&ground.heightfield())?;
        self.world.remove_body(std::mem::replace(&mut self.ground_patch, patch));
        let mesh = scene.insert(ground_mesh_instance(ground)?);
        scene.remove(std::mem::replace(&mut self.ground_mesh, mesh));
        Ok(())
    }

    /// Release chunks as debris: copies of their meshes on world-owned box bodies. Chunks already released are skipped.
    ///
    /// Directly fractured chunks (`apply_impact_impulse`) lose a thin outer shell, in mesh and collider alike, so they can
    /// leave a tightly fitted wall, and receive an impact velocity and spin. Support failures keep all but a sliver of
    /// clearance and start at rest.
    pub fn spawn<'a>(
        &mut self,
        scene: &mut RenderScene,
        chunks: impl IntoIterator<Item = &'a DestructibleChunkDefinition>,
        apply_impact_impulse: bool,
    ) -> AppResult {
        let release = if apply_impact_impulse { Release::Fractured } else { Release::Resting };
        self.release(scene, chunks, release)
    }

    /// Release chunks that failed under load: crushed to the fractured size, leaning out of the wall face (+Z) with a
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
            if self.pieces.iter().any(|(id, _)| *id == chunk.id) {
                continue;
            }
            // Full-size boxes wedge between the surviving chunks of a tightly fitted wall; every release shrinks a
            // little, and fractures most.
            let scale = match release {
                Release::Fractured | Release::Crumbled => DESTRUCTION.fractured_chunk_scale,
                Release::Resting => DESTRUCTION.released_chunk_scale,
            };
            let size = chunk.size.map(|value| value * scale);
            let center = chunk.center.as_vec3();
            let body = self.world.create_body(BodyOptions {
                mass: DESTRUCTION.debris_mass,
                position: center,
                half_extents: size.as_vec3() * 0.5,
                friction: DESTRUCTION.debris_friction,
                restitution: DESTRUCTION.debris_restitution,
                linear_damping: DESTRUCTION.debris_linear_damping,
                angular_damping: DESTRUCTION.debris_angular_damping,
                ..BodyOptions::new(PrimitiveShape::Box)
            })?;
            let top = center + Vec3::new(0.0, (chunk.size[1] / 2.0) as f32, 0.0);
            if release == Release::Crumbled {
                self.world.set_body_velocity(body, Vec3::new(0.0, 0.0, DESTRUCTION.crumble_lean_speed))?;
                self.world.apply_impulse(body, Vec3::from_array(DESTRUCTION.crumble_tip_impulse), Some(top))?;
            }
            if release == Release::Fractured {
                // Pieces fly away from the wall's middle, varied by release order.
                let direction = if chunk.center[0] < 0.0 { -1.0 } else { 1.0 };
                let count = self.pieces.len();
                let [launch_x, launch_y, launch_z] = DESTRUCTION.debris_launch_speed;
                let [vary_x, vary_y] = DESTRUCTION.debris_launch_variation;
                let velocity = Vec3::new(
                    direction * (launch_x + ((count % 3) as f32 * vary_x)),
                    launch_y + ((count % 4) as f32 * vary_y),
                    launch_z,
                );
                self.world.set_body_velocity(body, velocity)?;
                self.world.apply_impulse(body, Vec3::from_array(DESTRUCTION.debris_spin_impulse), Some(top))?;
            }
            let local = DestructibleChunkDefinition {
                size,
                center: DVec3::ZERO,
                ..chunk.clone()
            };
            let asset = MeshAsset::new(&create_chunk_mesh_data([&local]))?;
            let mesh = MeshInstance::new(asset).with_transform(Transform::new(center, Vec3::ZERO, Vec3::ONE));
            let mesh = scene.insert(mesh);
            self.pieces.push((
                chunk.id.clone(),
                DebrisPiece {
                    body,
                    mesh,
                },
            ));
        }
        Ok(())
    }

    /// Match static colliders to the committed structure before releasing chunks into rigid-body simulation, so
    /// surviving walls catch falling debris and removed colliders never overlap their new dynamic bodies.
    pub fn synchronize_structure<'a>(
        &mut self,
        chunks: impl IntoIterator<Item = &'a DestructibleChunkDefinition>,
    ) -> Result<(), PhysicsError> {
        let chunks: Vec<&DestructibleChunkDefinition> = chunks.into_iter().collect();
        let world = &mut self.world;
        self.static_chunks.retain(|(id, body)| {
            let keep = chunks.iter().any(|chunk| chunk.id == *id);
            if !keep {
                world.remove_body(*body);
            }
            keep
        });
        for chunk in chunks {
            if self.static_chunks.iter().any(|(id, _)| *id == chunk.id) {
                continue;
            }
            let body = self.world.create_body(BodyOptions {
                position: chunk.center.as_vec3(),
                half_extents: chunk.size.as_vec3() * 0.5,
                friction: DESTRUCTION.debris_friction,
                restitution: DESTRUCTION.debris_restitution,
                ..BodyOptions::new(PrimitiveShape::Box)
            })?;
            self.static_chunks.push((chunk.id.clone(), body));
        }
        Ok(())
    }

    /// Advance bounded fixed-step physics by `elapsed_seconds` and copy body poses into the scene.
    pub fn update(&mut self, scene: &mut RenderScene, elapsed_seconds: f32) -> Result<(), PhysicsError> {
        self.world.step(elapsed_seconds)?;
        for (_, piece) in &self.pieces {
            if let (Some(pose), Some(mesh)) = (self.world.body_transform(piece.body), scene.get_mut(piece.mesh)) {
                mesh.transform.position = pose.position;
                mesh.transform.rotation = pose.rotation;
            }
        }
        Ok(())
    }

    /// Copied diagnostics.
    #[allow(dead_code, reason = "part of the capture and test API that the integration tests drive")]
    pub fn snapshot(&self) -> DebrisSnapshot {
        DebrisSnapshot {
            ground_top: GROUND.height,
            static_chunks: self.static_chunks.len(),
            pieces: self
                .pieces
                .iter()
                .map(|(id, piece)| DebrisPieceSnapshot {
                    chunk_id: id.clone(),
                    position: self.world.body_transform(piece.body).map_or(Vec3::ZERO, |pose| pose.position),
                    velocity: self.world.body_velocity(piece.body).unwrap_or(Vec3::ZERO),
                })
                .collect(),
        }
    }

    /// Debris pieces retained.
    pub fn count(&self) -> usize {
        self.pieces.len()
    }

    /// Release every debris body and mesh, keeping the ground for the next reset.
    pub fn reset(&mut self, scene: &mut RenderScene) {
        for (_, piece) in self.pieces.drain(..) {
            self.world.remove_body(piece.body);
            scene.remove(piece.mesh);
        }
    }

    /// Release the ground mesh and every debris mesh from `scene`; the world and its bodies drop with `self`.
    #[allow(dead_code, reason = "part of the capture and test API that the integration tests drive")]
    pub fn dispose(mut self, scene: &mut RenderScene) {
        self.reset(scene);
        scene.remove(self.ground_mesh);
    }
}

/// The ground's mesh, in its own neutral tint, independent of the chunk material palette.
fn ground_mesh_instance(ground: &CraterGround) -> AppResult<MeshInstance> {
    Ok(MeshInstance::new(MeshAsset::new(&ground.mesh_data())?).with_tint(GROUND.tint))
}
