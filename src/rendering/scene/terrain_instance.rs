// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scene-owned presentation state for one resident terrain. The heightfield asset is shared; the renderer owns the
//! device-specific GPU resources and releases them when the instance leaves the scene.

use crate::core::Transform;

use crate::rendering::mesh::Bounds;
use crate::rendering::terrain::{
    TerrainAsset,
    TerrainDiagnostic,
    TerrainError,
};

/// Default screen-space error tolerance in CSS pixels.
pub const DEFAULT_TERRAIN_TOLERANCE: f32 = 2.0;

/// Presentation state for one drawn terrain.
#[derive(Clone, Debug)]
pub struct TerrainInstance {
    /// Shared resident heightfield.
    pub asset: TerrainAsset,
    /// World-space transform (scale, rotate X/Y/Z, translate) applied to the centered heightfield.
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
    pub(crate) parent: Option<crate::rendering::scene::SceneNodeId>,
}

impl TerrainInstance {
    /// An identity-transformed, visible terrain that casts and receives shadows, with the default tolerance.
    pub fn new(asset: TerrainAsset) -> Self {
        Self {
            asset,
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

    /// Local-space bounds: the centered extents and the asset's height range.
    pub fn local_bounds(&self) -> Bounds {
        self.asset.bounds()
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
