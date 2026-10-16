// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Persistent rigid debris for released chunks (TypeScript `destruction-debris.ts`): the ground slab, one engine physics
//! body and mesh per released chunk, and static colliders that keep the surviving structure solid.
//!
//! Physics uses meters, seconds, and Y-up world space. Debris persists until reset: no expiry or eviction, and one body
//! per chunk id bounds the example to its 126 generated chunks. Scenes are borrowed per call, never held.

use joy_engine::core::PrimitiveShape;
use joy_engine::physics::{
    BodyHandle, BodyOptions, DestructibleChunkDefinition, PhysicsError, PhysicsWorld, PhysicsWorldOptions,
    create_chunk_mesh_data,
};
use joy_engine::prelude::*;

use super::config::destruction_config::{DESTRUCTION, GROUND};

/// Material name of the ground's chunk record (only its geometry is used).
const GROUND_MATERIAL: &str = "frame";
/// Scene roughness of debris and the ground, for reflections.
const DEBRIS_ROUGHNESS: f32 = 0.7;

/// One released chunk: its dynamic body and the mesh that follows it.
struct DebrisPiece {
    body: BodyHandle,
    mesh: MeshInstanceId,
}

/// Copied motion of one debris body, in meters and meters per second.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code, reason = "part of the capture and test API that test/rust drives")]
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
#[allow(dead_code, reason = "part of the capture and test API that test/rust drives")]
pub struct DebrisSnapshot {
    /// World height of the ground's top.
    pub ground_top: f64,
    /// Static colliders matching surviving chunks.
    pub static_chunks: usize,
    /// Every debris body, in release order.
    pub pieces: Vec<DebrisPieceSnapshot>,
}

/// Owns the physics world, the ground mesh, debris, and static chunk colliders.
pub struct DestructionDebris {
    world: PhysicsWorld,
    #[allow(dead_code, reason = "part of the capture and test API that test/rust drives")]
    ground_mesh: MeshInstanceId,
    // Released chunks and surviving-chunk colliders by chunk id, in insertion order (at most one per chunk).
    pieces: Vec<(String, DebrisPiece)>,
    static_chunks: Vec<(String, BodyHandle)>,
}

impl DestructionDebris {
    /// Create the world and the ground slab (collider and mesh, sharing dimensions) in `scene`.
    pub fn new(scene: &mut RenderScene) -> GameResult<Self> {
        let mut world = PhysicsWorld::new(PhysicsWorldOptions {
            gravity: Vec3::new(0.0, -DESTRUCTION.debris_gravity, 0.0),
            fixed_step: DESTRUCTION.physics_step_seconds,
            max_sub_steps: DESTRUCTION.maximum_physics_substeps,
        })?;
        world.create_body(BodyOptions {
            position: vector(GROUND.center),
            half_extents: vector(GROUND.size) * 0.5,
            friction: GROUND.friction,
            restitution: GROUND.restitution,
            ..BodyOptions::new(PrimitiveShape::Box)
        })?;
        let ground = DestructibleChunkDefinition {
            id: "ground".to_owned(),
            center: GROUND.center,
            size: GROUND.size,
            material: GROUND_MATERIAL.to_owned(),
            hit_points: 1.0,
        };
        // The ground uses its own neutral tint, independent of the chunk material palette.
        let mut geometry = create_chunk_mesh_data([&ground]);
        geometry.colors = None;
        let mut mesh = MeshInstance::new(MeshAsset::from_asset_data(&geometry)?).with_tint(GROUND.tint);
        mesh.scene_roughness = 1.0;
        let ground_mesh = scene.insert(mesh);
        Ok(Self { world, ground_mesh, pieces: Vec::new(), static_chunks: Vec::new() })
    }

    /// Release chunks as debris: copies of their meshes on world-owned box bodies. Chunks already released are skipped.
    ///
    /// Directly fractured chunks (`apply_impact_impulse`) lose a thin outer shell, in mesh and collider alike, so they can
    /// leave a tightly fitted wall, and receive an impact velocity and spin. Support failures keep their size and start at
    /// rest.
    pub fn spawn<'a>(
        &mut self,
        scene: &mut RenderScene,
        chunks: impl IntoIterator<Item = &'a DestructibleChunkDefinition>,
        apply_impact_impulse: bool,
    ) -> GameResult {
        for chunk in chunks {
            if self.pieces.iter().any(|(id, _)| *id == chunk.id) {
                continue;
            }
            let scale = if apply_impact_impulse { DESTRUCTION.fractured_chunk_scale } else { 1.0 };
            let size = chunk.size.map(|value| value * scale);
            let center = vector(chunk.center);
            let body = self.world.create_body(BodyOptions {
                mass: DESTRUCTION.debris_mass,
                position: center,
                half_extents: vector(size) * 0.5,
                friction: DESTRUCTION.debris_friction,
                restitution: DESTRUCTION.debris_restitution,
                linear_damping: DESTRUCTION.debris_linear_damping,
                angular_damping: DESTRUCTION.debris_angular_damping,
                ..BodyOptions::new(PrimitiveShape::Box)
            })?;
            if apply_impact_impulse {
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
                let top = center + Vec3::new(0.0, (chunk.size[1] / 2.0) as f32, 0.0);
                self.world.apply_impulse(body, Vec3::from_array(DESTRUCTION.debris_spin_impulse), Some(top))?;
            }
            let local = DestructibleChunkDefinition { size, center: [0.0; 3], ..chunk.clone() };
            let asset = MeshAsset::from_asset_data(&create_chunk_mesh_data([&local]))?;
            let mut mesh = MeshInstance::new(asset).with_transform(Transform::new(center, Vec3::ZERO, Vec3::ONE));
            mesh.scene_roughness = DEBRIS_ROUGHNESS;
            let mesh = scene.insert(mesh);
            self.pieces.push((chunk.id.clone(), DebrisPiece { body, mesh }));
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
                position: vector(chunk.center),
                half_extents: vector(chunk.size) * 0.5,
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
    #[allow(dead_code, reason = "part of the capture and test API that test/rust drives")]
    pub fn snapshot(&self) -> DebrisSnapshot {
        DebrisSnapshot {
            ground_top: GROUND.center[1] + (GROUND.size[1] / 2.0),
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
    #[allow(dead_code, reason = "part of the capture and test API that test/rust drives")]
    pub fn dispose(mut self, scene: &mut RenderScene) {
        self.reset(scene);
        scene.remove(self.ground_mesh);
    }
}

/// A world-space vector from an `f64` triple.
fn vector(values: [f64; 3]) -> Vec3 {
    Vec3::new(values[0] as f32, values[1] as f32, values[2] as f32)
}
