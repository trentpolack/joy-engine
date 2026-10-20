// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Shared, immutable mesh assets.
//!
//! A [`MeshAsset`] copies and validates a [`Mesh`] once into interleaved `f32` vertices and groups its triangles into one
//! [`Submesh`] per material slot. Assets hold no GPU resources: the renderer uploads each asset the first time it draws
//! it and releases the upload once no scene references the asset. Removing an instance or dropping a scene never
//! invalidates an asset used elsewhere.
//!
//! Geometry and materials stay separate: an asset carries one default [`MaterialAsset`] per slot, and scene instances
//! may override any slot (see [`MeshInstance`](crate::MeshInstance)). Vertices hold geometry only, so one asset draws
//! with any material and any shading model.

use std::sync::Arc;
use std::sync::atomic::{
    AtomicU64,
    Ordering,
};

use bytemuck::{
    Pod,
    Zeroable,
};
use joy_core::LinearColor;
use joy_core::math::Vec3;
use joy_render_types::{
    Mesh,
    MeshDocument,
};

use crate::RenderError;
use crate::material::MaterialAsset;
use crate::texture::TextureAsset;

/// Source of process-unique asset identities used to key GPU uploads.
static NEXT_ASSET_ID: AtomicU64 = AtomicU64::new(1);

/// Interleaved GPU vertex shared by every mesh pass. 48 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct MeshVertex {
    /// Model-space position.
    pub position: [f32; 3],
    /// Unit model-space normal; zero where the mesh authored none, which shading models shade as faceted.
    pub normal: [f32; 3],
    /// Linear RGBA vertex color with straight alpha.
    pub color: [f32; 4],
    /// Texture coordinates with a top-left origin; repeat outside `[0, 1]`.
    pub uv: [f32; 2],
}

impl MeshVertex {
    /// A vertex with no authored normal or texture coordinates, as transient triangles draw.
    pub(crate) fn colored(position: [f32; 3], color: LinearColor) -> Self {
        Self {
            position,
            normal: [0.0; 3],
            color: color.to_array(),
            uv: [0.0; 2],
        }
    }
}

/// A contiguous index range drawn with one material slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submesh {
    /// Index range into the asset's index buffer.
    pub indices: std::ops::Range<u32>,
    /// The material slot the range draws with.
    pub slot: usize,
}

/// Axis-aligned bounds in the asset's local space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    /// Minimum corner.
    pub min: Vec3,
    /// Maximum corner.
    pub max: Vec3,
}

impl Bounds {
    /// Center point.
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// The eight corners, in bit order (x, y, z) from min to max.
    pub fn corners(&self) -> [Vec3; 8] {
        std::array::from_fn(|corner| {
            Vec3::new(
                if corner & 1 != 0 { self.max.x } else { self.min.x },
                if corner & 2 != 0 { self.max.y } else { self.min.y },
                if corner & 4 != 0 { self.max.z } else { self.min.z },
            )
        })
    }
}

/// Immutable, cheaply cloned mesh shared by any number of scene instances.
#[derive(Clone, Debug)]
pub struct MeshAsset {
    inner: Arc<MeshAssetInner>,
}

#[derive(Debug)]
struct MeshAssetInner {
    id: u64,
    vertices: Vec<MeshVertex>,
    indices: Vec<u32>,
    submeshes: Vec<Submesh>,
    materials: Vec<MaterialAsset>,
    bounds: Bounds,
    has_vertex_alpha: bool,
}

impl MeshAsset {
    /// Validate and copy `mesh`, drawing every slot with the default material (opaque white lit shading).
    ///
    /// Fails for meshes [`Mesh::validate`] rejects, and for positions or attributes that do not fit finite `f32`s.
    pub fn new(mesh: &Mesh) -> Result<Self, RenderError> {
        Self::with_materials(mesh, Vec::new())
    }

    /// Validate and copy `mesh` with `materials` as its slots' defaults, in slot order. Slots past the end of
    /// `materials` draw with the default material; extra materials are ignored.
    ///
    /// Triangles are regrouped by slot, keeping their order within each slot, and authored normals are normalized.
    pub fn with_materials(mesh: &Mesh, materials: Vec<MaterialAsset>) -> Result<Self, RenderError> {
        mesh.validate().map_err(|error| RenderError::InvalidMesh(error.to_string()))?;
        let vertices = interleave(mesh)?;
        let slot_count = mesh.slot_count();

        // One submesh per slot that has triangles, slots in order and triangles in authored order within each.
        let mut indices = Vec::with_capacity(mesh.indices.len());
        let mut submeshes = Vec::new();
        for slot in 0..slot_count {
            let start = indices.len() as u32;
            for (triangle, corners) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
                if mesh.triangle_slot(triangle) as usize == slot {
                    indices.extend_from_slice(corners);
                }
            }
            let end = indices.len() as u32;
            if end > start {
                submeshes.push(Submesh {
                    indices: start..end,
                    slot,
                });
            }
        }

        let mut slot_materials = materials;
        slot_materials.truncate(slot_count);
        while slot_materials.len() < slot_count {
            slot_materials.push(MaterialAsset::default());
        }

        let bounds = vertices.iter().fold(
            Bounds {
                min: Vec3::splat(f32::INFINITY),
                max: Vec3::splat(f32::NEG_INFINITY),
            },
            |bounds, vertex| {
                let position = Vec3::from_array(vertex.position);
                Bounds { min: bounds.min.min(position), max: bounds.max.max(position) }
            },
        );
        let has_vertex_alpha = vertices.iter().any(|vertex| vertex.color[3] < 1.0);
        let id = NEXT_ASSET_ID.fetch_add(1, Ordering::Relaxed);
        Ok(Self {
            inner: Arc::new(MeshAssetInner {
                id,
                vertices,
                indices,
                submeshes,
                materials: slot_materials,
                bounds,
                has_vertex_alpha,
            }),
        })
    }

    /// Validate a mesh document and build its asset, resolving each material's texture sources through `resolve` (see
    /// [`MaterialAsset::with_textures`]).
    pub fn from_document(
        document: &MeshDocument,
        mut resolve: impl FnMut(&str) -> Option<TextureAsset>,
    ) -> Result<Self, RenderError> {
        document.validate().map_err(|error| RenderError::InvalidMesh(error.to_string()))?;
        let materials = document
            .materials
            .iter()
            .map(|material| MaterialAsset::with_textures(material.clone(), &mut resolve))
            .collect::<Result<Vec<_>, _>>()?;
        Self::with_materials(&document.mesh, materials)
    }

    /// Process-unique identity used to key GPU uploads.
    pub fn id(&self) -> u64 {
        self.inner.id
    }

    /// The index ranges of each slot that has triangles, in slot order.
    pub fn submeshes(&self) -> &[Submesh] {
        &self.inner.submeshes
    }

    /// The default material of every slot, in slot order.
    pub fn materials(&self) -> &[MaterialAsset] {
        &self.inner.materials
    }

    /// Number of material slots.
    pub fn slot_count(&self) -> usize {
        self.inner.materials.len()
    }

    /// Local-space bounds.
    pub fn bounds(&self) -> Bounds {
        self.inner.bounds
    }

    /// Number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.inner.vertices.len()
    }

    /// Number of triangles.
    pub fn triangle_count(&self) -> usize {
        self.inner.indices.len() / 3
    }

    /// Whether any vertex has alpha below one; `RenderPassKind::Auto` instances then draw transparently.
    pub fn has_vertex_alpha(&self) -> bool {
        self.inner.has_vertex_alpha
    }

    /// Whether two handles refer to the same asset.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Interleaved vertices, for upload.
    pub(crate) fn vertices(&self) -> &[MeshVertex] {
        &self.inner.vertices
    }

    /// Triangle-list indices grouped by slot, for upload.
    pub(crate) fn indices(&self) -> &[u32] {
        &self.inner.indices
    }

    /// A type-erased weak reference used by the GPU cache to release uploads for dropped assets.
    pub(crate) fn downgrade(&self) -> std::sync::Weak<dyn std::any::Any + Send + Sync> {
        let strong: Arc<dyn std::any::Any + Send + Sync> = self.inner.clone();
        Arc::downgrade(&strong)
    }
}

/// Interleave a validated mesh's attributes as `f32` vertices: colors default to white, texture coordinates to zero,
/// and authored normals are normalized (zero normals stay zero, which shades faceted).
fn interleave(mesh: &Mesh) -> Result<Vec<MeshVertex>, RenderError> {
    let mut vertices = Vec::with_capacity(mesh.positions.len());
    for (index, position) in mesh.positions.iter().enumerate() {
        let position = position.as_vec3();
        if !position.is_finite() {
            return Err(RenderError::InvalidMesh("positions must fit finite float32 values".into()));
        }
        let normal = mesh.normals.as_ref().map_or(Vec3::ZERO, |normals| normals[index]);
        let normal = if normal.length_squared() > 0.0 { normal.normalize_or_zero() } else { Vec3::ZERO };
        let color = mesh.colors.as_ref().map_or(LinearColor::WHITE, |colors| colors[index]);
        let uv = mesh.uvs.as_ref().map_or([0.0; 2], |uvs| uvs[index].to_array());
        vertices.push(MeshVertex {
            position: position.to_array(),
            normal: normal.to_array(),
            color: color.to_array(),
            uv,
        });
    }
    Ok(vertices)
}

#[cfg(test)]
#[path = "mesh.test.rs"]
mod tests;
