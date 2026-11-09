// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! One draw model for every pass: opaque batches, blended meshes, shadow casters, terrain, transient triangles, and
//! particle batches are all [`DrawItem`]s, and each pass draws a queue of them in order.
//!
//! An item names its geometry, the shading model and material it draws with, and the instance range it draws. The
//! renderer resolves the pipeline from the item's geometry and the pass, so passes no longer special-case terrain or
//! transient triangles: terrain is an item that shades with the lit model (its G-buffer surfaces are lit by the lit
//! model's lighting pass and stamp its stencil slot), and transient triangles are items that draw a range of the
//! frame's transient vertex stream with the identity instance and the renderer's default materials. Terrain keeps its
//! own pipelines because its vertex stage morphs resident tiles rather than reading mesh vertices, so it cannot compile
//! through the shading-model registry; the [`OWN_MATERIAL`] identity stands in for its material group, which the
//! terrain adapter binds itself, as particle batches bind their sprites.

use std::ops::Range;

use crate::material::ModelId;

/// The material identity of items that bind their own material-slot group (terrain and particle batches); material
/// assets number from one, so no asset shares it.
pub(crate) const OWN_MATERIAL: u64 = 0;

/// What one draw item draws.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DrawGeometry {
    /// One submesh of an uploaded mesh asset: its index range.
    Mesh {
        /// The asset's identity (`MeshAsset::id`), which keys its GPU copy.
        asset: u64,
        indices: Range<u32>,
    },
    /// A vertex range of the frame's transient triangle stream.
    Transient { vertices: Range<u32> },
    /// The terrain tiles the pass's view reaches (the camera's, or one shadow view's).
    Terrain,
    /// One prepared particle batch, by its index in the particle pass's batches.
    Particles { batch: usize },
}

/// One draw in a pass queue.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DrawItem {
    pub geometry: DrawGeometry,
    /// The shading model the item draws with; with deferred lighting, the model whose lighting pass lights it.
    pub model: ModelId,
    /// The material whose group the item binds (`MaterialAsset::id`), or [`OWN_MATERIAL`].
    pub material: u64,
    /// The instance range it draws from the frame's instance buffer.
    pub instances: Range<u32>,
    /// Whether depth-only passes alpha-test it through its model.
    pub masked: bool,
    /// Whether it shades in the G-buffer pass when deferred lighting runs.
    pub deferred: bool,
}

impl DrawItem {
    /// Whether the item draws a mesh submesh (rather than terrain, transient triangles, or particles).
    pub(crate) fn is_mesh(&self) -> bool {
        matches!(self.geometry, DrawGeometry::Mesh { .. })
    }

    /// Triangles one draw of this item submits, when the item knows them (terrain and particles count their own).
    pub(crate) fn triangles(&self) -> usize {
        let instances = self.instances.len();
        match &self.geometry {
            DrawGeometry::Mesh {
                indices,
                ..
            } => (indices.len() / 3) * instances,
            DrawGeometry::Transient {
                vertices,
            } => vertices.len() / 3,
            DrawGeometry::Terrain
            | DrawGeometry::Particles {
                ..
            } => 0,
        }
    }
}

/// The frame's opaque queues: the same items in depth order (front to back, for the depth pre-pass) and in shading
/// order (for the G-buffer and scene passes).
#[derive(Clone, Debug, Default)]
pub(crate) struct OpaqueQueues {
    /// Terrain first (the largest occluder), then mesh batches front to back by their nearest instance, then transient
    /// triangles.
    pub depth_order: Vec<DrawItem>,
    /// The same items in shading order: after the depth pre-pass only the nearest surface passes the depth test, so
    /// mesh batches group by model and material (nearest first within each) to save state changes; without it they keep
    /// their front-to-back order. Terrain stays first and transient triangles last.
    pub shading_order: Vec<DrawItem>,
}

impl OpaqueQueues {
    /// Build both orders from `depth_order`.
    pub(crate) fn new(depth_order: Vec<DrawItem>, depth_prepass: bool) -> Self {
        let mut shading_order = depth_order.clone();
        if depth_prepass {
            // Only mesh batches regroup; the sort is stable, so equal keys keep their front-to-back order.
            let meshes = shading_order.iter().position(DrawItem::is_mesh).unwrap_or(shading_order.len());
            let end = shading_order.iter().rposition(DrawItem::is_mesh).map_or(meshes, |last| last + 1);
            shading_order[meshes..end].sort_by_key(|item| (item.model, item.material));
        }
        Self {
            depth_order,
            shading_order,
        }
    }

    /// Mesh batches that shade forward while deferred lighting runs.
    pub(crate) fn forward_batches(&self) -> usize {
        self.depth_order.iter().filter(|item| item.is_mesh() && !item.deferred).count()
    }

    /// Whether any item writes the G-buffer when deferred lighting runs.
    pub(crate) fn has_deferred(&self) -> bool {
        self.depth_order.iter().any(|item| item.deferred)
    }

    /// The models whose surfaces write the G-buffer, in registry order: one lighting pass each.
    pub(crate) fn deferred_models(&self) -> Vec<ModelId> {
        let mut models: Vec<ModelId> =
            self.depth_order.iter().filter(|item| item.deferred).map(|item| item.model).collect();
        models.sort_unstable();
        models.dedup();
        models
    }
}
