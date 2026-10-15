// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Shared, immutable mesh assets.
//!
//! A `MeshAsset` copies and validates indexed local-space geometry once. Assets hold no GPU resources; the renderer
//! uploads each asset the first time it draws it and releases the upload once no scene references the asset. Removing
//! an instance or dropping a scene never invalidates an asset used elsewhere.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use crate::RenderError;

/// Source of process-unique asset identities used to key GPU uploads.
static NEXT_ASSET_ID: AtomicU64 = AtomicU64::new(1);

/// Default roughness for geometry without an authored material: fully rough, so the PBR path reads as diffuse.
pub const DEFAULT_ROUGHNESS: f32 = 1.0;

/// Material transparency handling, matching FORM's PBR alpha modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AlphaMode {
    /// Alpha is ignored.
    #[default]
    Opaque,
    /// Fragments below the cutoff are discarded; survivors are opaque.
    Mask,
    /// Straight-alpha source-over blending, sorted back to front.
    Blend,
}

impl AlphaMode {
    /// Shader encoding stored in the vertex surface channel.
    fn shader_value(self) -> f32 {
        match self {
            Self::Opaque => 0.0,
            Self::Mask => 1.0,
            Self::Blend => 2.0,
        }
    }
}

/// Interleaved GPU vertex shared by every surface material. 72 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct MeshVertex {
    /// Local-space position.
    pub position: [f32; 3],
    /// Local-space normal; zero requests faceted derivative normals.
    pub normal: [f32; 3],
    /// Linear RGBA vertex color (material base color is already applied).
    pub color: [f32; 4],
    /// Metallic, roughness, alpha cutoff, and alpha mode.
    pub surface: [f32; 4],
    /// Linear RGB emission; the fourth component is reserved.
    pub emissive: [f32; 4],
}

/// Per-vertex material factors baked into the vertex stream.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VertexMaterial {
    /// Metallic factor in `[0, 1]`.
    pub metallic: f32,
    /// Perceptual roughness in `[0, 1]`.
    pub roughness: f32,
    /// Linear RGB emission.
    pub emissive: [f32; 3],
    /// Transparency handling.
    pub alpha_mode: AlphaMode,
    /// Cutoff used by `AlphaMode::Mask`.
    pub alpha_cutoff: f32,
}

impl Default for VertexMaterial {
    fn default() -> Self {
        Self {
            metallic: 0.0,
            roughness: DEFAULT_ROUGHNESS,
            emissive: [0.0; 3],
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
        }
    }
}

impl MeshVertex {
    /// Build a vertex from its channels and baked material.
    pub fn new(position: [f32; 3], normal: [f32; 3], color: [f32; 4], material: VertexMaterial) -> Self {
        Self {
            position,
            normal,
            color,
            surface: [material.metallic, material.roughness, material.alpha_cutoff, material.alpha_mode.shader_value()],
            emissive: [material.emissive[0], material.emissive[1], material.emissive[2], 0.0],
        }
    }

    /// Whether this vertex belongs to a blended surface.
    fn blends(&self) -> bool {
        self.surface[3] > 1.5
    }
}

/// A contiguous index range drawn with one alpha mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submesh {
    /// Index range into `MeshData::indices`.
    pub indices: std::ops::Range<u32>,
    /// Whether this range blends (and therefore draws in the sorted transparent queue).
    pub blend: bool,
}

/// Validated indexed triangle geometry in local space.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    /// Interleaved vertices.
    pub vertices: Vec<MeshVertex>,
    /// Triangle list indices into `vertices`.
    pub indices: Vec<u32>,
}

/// Plain channel arrays accepted by [`MeshAsset::from_channels`], mirroring the TypeScript `MeshAssetData`.
#[derive(Clone, Debug, Default)]
pub struct MeshChannels<'a> {
    /// Packed XYZ positions.
    pub positions: &'a [f32],
    /// Triangle list indices.
    pub indices: &'a [u32],
    /// Packed XYZ normals; empty for faceted shading.
    pub normals: &'a [f32],
    /// Packed linear RGB colors; empty for white.
    pub colors: &'a [f32],
    /// Per-vertex alpha; empty for opaque.
    pub alphas: &'a [f32],
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
    data: MeshData,
    submeshes: Vec<Submesh>,
    bounds: Bounds,
    has_vertex_alpha: bool,
}

impl MeshAsset {
    /// Validate and take ownership of interleaved geometry.
    ///
    /// Triangles are regrouped so opaque and blended ranges are contiguous; the relative order within each group is
    /// preserved. Fails for empty geometry, incomplete triangles, out-of-range indices, or non-finite positions.
    pub fn new(data: MeshData) -> Result<Self, RenderError> {
        validate(&data)?;

        // Partition triangles by blend state so each group draws with one pipeline.
        let mut opaque = Vec::with_capacity(data.indices.len());
        let mut blended = Vec::new();
        for triangle in data.indices.chunks_exact(3) {
            let blends = triangle.iter().any(|index| data.vertices[*index as usize].blends());
            if blends {
                blended.extend_from_slice(triangle);
            } else {
                opaque.extend_from_slice(triangle);
            }
        }
        let opaque_count = opaque.len() as u32;
        let total = (opaque.len() + blended.len()) as u32;
        let mut submeshes = Vec::new();
        if opaque_count > 0 {
            submeshes.push(Submesh { indices: 0..opaque_count, blend: false });
        }
        if total > opaque_count {
            submeshes.push(Submesh { indices: opaque_count..total, blend: true });
        }
        opaque.extend(blended);

        let bounds = data.vertices.iter().fold(
            Bounds { min: Vec3::splat(f32::INFINITY), max: Vec3::splat(f32::NEG_INFINITY) },
            |bounds, vertex| {
                let position = Vec3::from_array(vertex.position);
                Bounds { min: bounds.min.min(position), max: bounds.max.max(position) }
            },
        );
        let has_vertex_alpha = data.vertices.iter().any(|vertex| vertex.color[3] < 1.0);
        let id = NEXT_ASSET_ID.fetch_add(1, Ordering::Relaxed);
        let data = MeshData { vertices: data.vertices, indices: opaque };
        Ok(Self { inner: Arc::new(MeshAssetInner { id, data, submeshes, bounds, has_vertex_alpha }) })
    }

    /// Build an asset from plain channel arrays with default (fully rough, opaque) material factors.
    ///
    /// Colors default to white and alpha to one. Normals may be empty for faceted shading.
    pub fn from_channels(channels: MeshChannels<'_>) -> Result<Self, RenderError> {
        let vertex_count = channels.positions.len() / 3;
        if !channels.positions.len().is_multiple_of(3) {
            return Err(RenderError::InvalidMesh("positions must contain complete XYZ vertices".into()));
        }
        let check = |name: &str, length: usize, stride: usize| -> Result<(), RenderError> {
            if length != 0 && length != vertex_count * stride {
                return Err(RenderError::InvalidMesh(format!("{name} must match the vertex count")));
            }
            Ok(())
        };
        check("normals", channels.normals.len(), 3)?;
        check("colors", channels.colors.len(), 3)?;
        check("alphas", channels.alphas.len(), 1)?;

        let vertices = (0..vertex_count)
            .map(|index| {
                let triple = |values: &[f32], fallback: f32| {
                    if values.is_empty() {
                        [fallback; 3]
                    } else {
                        [values[index * 3], values[(index * 3) + 1], values[(index * 3) + 2]]
                    }
                };
                let position = triple(channels.positions, 0.0);
                let normal = triple(channels.normals, 0.0);
                let color = triple(channels.colors, 1.0);
                let alpha = channels.alphas.get(index).copied().unwrap_or(1.0);
                let material = VertexMaterial {
                    alpha_mode: if alpha < 1.0 { AlphaMode::Blend } else { AlphaMode::Opaque },
                    ..VertexMaterial::default()
                };
                MeshVertex::new(position, normal, [color[0], color[1], color[2], alpha], material)
            })
            .collect();
        Self::new(MeshData { vertices, indices: channels.indices.to_vec() })
    }

    /// Process-unique identity used to key GPU uploads.
    pub fn id(&self) -> u64 {
        self.inner.id
    }

    /// Validated geometry, with opaque triangles before blended ones.
    pub fn data(&self) -> &MeshData {
        &self.inner.data
    }

    /// Contiguous opaque and blended index ranges.
    pub fn submeshes(&self) -> &[Submesh] {
        &self.inner.submeshes
    }

    /// Local-space bounds.
    pub fn bounds(&self) -> Bounds {
        self.inner.bounds
    }

    /// Whether any vertex has alpha below one; `RenderPassKind::Auto` instances then draw transparently.
    pub fn has_vertex_alpha(&self) -> bool {
        self.inner.has_vertex_alpha
    }

    /// Whether two handles refer to the same asset.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// A type-erased weak reference used by the GPU cache to release uploads for dropped assets.
    pub(crate) fn downgrade(&self) -> std::sync::Weak<dyn std::any::Any + Send + Sync> {
        let strong: Arc<dyn std::any::Any + Send + Sync> = self.inner.clone();
        Arc::downgrade(&strong)
    }
}

/// Reject geometry the GPU path cannot draw safely.
fn validate(data: &MeshData) -> Result<(), RenderError> {
    if data.vertices.is_empty() || data.indices.is_empty() {
        return Err(RenderError::InvalidMesh("geometry needs at least one vertex and one triangle".into()));
    }
    if !data.indices.len().is_multiple_of(3) {
        return Err(RenderError::InvalidMesh("indices must form complete triangles".into()));
    }
    let vertex_count = data.vertices.len() as u32;
    if let Some(index) = data.indices.iter().find(|index| **index >= vertex_count) {
        return Err(RenderError::InvalidMesh(format!("index {index} exceeds {vertex_count} vertices")));
    }
    let finite = data
        .vertices
        .iter()
        .all(|vertex| vertex.position.iter().chain(&vertex.normal).chain(&vertex.color).all(|value| value.is_finite()));
    if !finite {
        return Err(RenderError::InvalidMesh("vertex channels must be finite".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triangle(alpha: f32) -> MeshAsset {
        MeshAsset::from_channels(MeshChannels {
            positions: &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            indices: &[0, 1, 2],
            alphas: &[alpha, alpha, alpha],
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn assets_validate_and_partition_blending() {
        let opaque = triangle(1.0);
        assert_eq!(opaque.submeshes(), &[Submesh { indices: 0..3, blend: false }]);
        assert!(!opaque.has_vertex_alpha());

        let blended = triangle(0.5);
        assert_eq!(blended.submeshes(), &[Submesh { indices: 0..3, blend: true }]);
        assert!(blended.has_vertex_alpha());
        assert_ne!(opaque.id(), blended.id());
    }

    #[test]
    fn malformed_geometry_is_rejected() {
        let result = MeshAsset::from_channels(MeshChannels {
            positions: &[0.0, 0.0, 0.0],
            indices: &[0, 1, 2],
            ..Default::default()
        });
        assert!(matches!(result, Err(RenderError::InvalidMesh(_))));
    }
}
