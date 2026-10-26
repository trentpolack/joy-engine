// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scene-owned presentation state for one terrain. The surface snapshot is shared (a streamer publishes new snapshots
//! into the instance as tiles arrive); the renderer owns the device-specific GPU tile pool and releases it when the
//! instance leaves the scene or its surface changes lineage.

use joy_core::Transform;
use joy_core::math::Vec3;
use joy_terrain::{
    TerrainError,
    TerrainSurface,
};

use crate::mesh::Bounds;
use crate::terrain::TerrainDiagnostic;

/// Default screen-space error tolerance in CSS pixels.
pub const DEFAULT_TERRAIN_TOLERANCE: f32 = 2.0;

/// Presentation state for one drawn terrain.
#[derive(Clone, Debug)]
pub struct TerrainInstance {
    /// The resident tiles to draw. Replace it with each new snapshot of the same terrain; a surface of another
    /// lineage starts a fresh GPU tile pool.
    pub surface: TerrainSurface,
    /// World-space transform (scale, rotate X/Y/Z, translate) applied to the surface's local space.
    pub transform: Transform,
    /// Hidden terrain neither draws nor casts shadows, and the renderer releases its GPU resources.
    pub visible: bool,
    /// Whether the terrain writes into the directional shadow map.
    pub casts_shadow: bool,
    /// Whether lighting samples the directional shadow map.
    pub receives_shadow: bool,
    /// Screen-space error tolerance in CSS pixels; finite and positive.
    pub tolerance: f32,
    /// World-unit radius around the camera focus pinned to the finest detail; finite and nonnegative, zero disables.
    pub target_radius: f32,
    /// Shading mode.
    pub diagnostic: TerrainDiagnostic,
    /// Scene node this terrain inherits transform and visibility from; set through `RenderScene::set_terrain_parent`.
    pub(crate) parent: Option<crate::scene::SceneNodeId>,
}

impl TerrainInstance {
    /// An identity-transformed, visible terrain that casts and receives shadows, with the default tolerance.
    pub fn new(surface: TerrainSurface) -> Self {
        Self {
            surface,
            transform: Transform::IDENTITY,
            visible: true,
            casts_shadow: true,
            receives_shadow: true,
            tolerance: DEFAULT_TERRAIN_TOLERANCE,
            target_radius: 0.0,
            diagnostic: TerrainDiagnostic::Material,
            parent: None,
        }
    }

    /// Builder-style transform assignment.
    pub fn with_transform(mut self, transform: Transform) -> Self {
        self.transform = transform;
        self
    }

    /// Local-space bounds: the surface footprint and every height its resident levels reach.
    pub fn local_bounds(&self) -> Bounds {
        let bounds = self.surface.bounds();
        Bounds {
            min: Vec3::new(bounds.min.x as f32, bounds.min.y as f32, bounds.min.z as f32),
            max: Vec3::new(bounds.max.x as f32, bounds.max.y as f32, bounds.max.z as f32),
        }
    }

    /// Check the LOD policy; the renderer calls this before preparing the instance.
    pub fn validate(&self) -> Result<(), TerrainError> {
        if !self.tolerance.is_finite() || self.tolerance <= 0.0 {
            return Err(TerrainError::InvalidInstance("Terrain tolerance must be finite and positive."));
        }
        if !self.target_radius.is_finite() || self.target_radius < 0.0 {
            return Err(TerrainError::InvalidInstance("Terrain target radius must be finite and nonnegative."));
        }
        Ok(())
    }
}
