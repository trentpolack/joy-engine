// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Shared, immutable mesh assets.
//!
//! A `MeshAsset` copies and validates indexed local-space geometry once. Assets hold no GPU resources; the renderer
//! uploads each asset the first time it draws it and releases the upload once no scene references the asset. Removing
//! an instance or dropping a scene never invalidates an asset used elsewhere.
//!
//! Material factors are baked into vertices. Submeshes may also carry [`MaterialTextures`] (base color, normal,
//! emissive, metallic, roughness, and occlusion maps) that multiply those factors in the PBR pass;
//! [`MeshAsset::from_pbr_mesh`] builds such assets from the TypeScript `PbrMesh` document.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use crate::core::{MaterialAlphaMode, MeshAssetData, PbrMaterial, PbrMesh};
use crate::rendering::RenderError;
use crate::rendering::texture::TextureAsset;

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

/// Interleaved GPU vertex shared by every surface material. 80 bytes.
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
    /// Texture coordinates with a top-left origin; repeat outside `[0, 1]`.
    pub uv: [f32; 2],
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
            uv: [0.0; 2],
        }
    }

    /// This vertex with texture coordinates.
    pub fn with_uv(mut self, uv: [f32; 2]) -> Self {
        self.uv = uv;
        self
    }

    /// Whether this vertex belongs to a blended surface.
    fn blends(&self) -> bool {
        self.surface[3] > 1.5
    }

    /// Whether this vertex belongs to an alpha-tested surface.
    fn masks(&self) -> bool {
        (0.5..1.5).contains(&self.surface[3])
    }
}

/// Texture maps for one material; missing maps behave as white (or a flat normal). Color maps (base color and
/// emission) are sRGB; normal maps are tangent-space XYZ; metallic, roughness, and occlusion read the red channel.
#[derive(Clone, Debug, Default)]
pub struct TextureMaps {
    /// Base color and alpha, multiplied with the vertex color.
    pub base_color: Option<TextureAsset>,
    /// Tangent-space normal map.
    pub normal: Option<TextureAsset>,
    /// Emission, multiplied with the emissive factor.
    pub emissive: Option<TextureAsset>,
    /// Metalness, multiplied with the metallic factor.
    pub metallic: Option<TextureAsset>,
    /// Roughness, multiplied with the roughness factor.
    pub roughness: Option<TextureAsset>,
    /// Ambient occlusion, applied to ambient and image-based light.
    pub ao: Option<TextureAsset>,
}

impl TextureMaps {
    /// Whether no map is set.
    pub fn is_empty(&self) -> bool {
        self.textures().all(|texture| texture.is_none())
    }

    fn textures(&self) -> impl Iterator<Item = &Option<TextureAsset>> {
        [&self.base_color, &self.normal, &self.emissive, &self.metallic, &self.roughness, &self.ao].into_iter()
    }
}

/// Shared, immutable texture maps for PBR submeshes. Handles compare equal when they share one allocation.
#[derive(Clone, Debug)]
pub struct MaterialTextures {
    inner: Arc<MaterialTexturesInner>,
}

#[derive(Debug)]
struct MaterialTexturesInner {
    id: u64,
    maps: TextureMaps,
}

impl MaterialTextures {
    /// Share a set of maps.
    pub fn new(maps: TextureMaps) -> Self {
        let id = NEXT_ASSET_ID.fetch_add(1, Ordering::Relaxed);
        Self { inner: Arc::new(MaterialTexturesInner { id, maps }) }
    }

    /// Process-unique identity used to key GPU bindings.
    pub fn id(&self) -> u64 {
        self.inner.id
    }

    /// The maps.
    pub fn maps(&self) -> &TextureMaps {
        &self.inner.maps
    }

    /// A type-erased weak reference used by the GPU cache to release bindings for dropped materials.
    pub(crate) fn downgrade(&self) -> std::sync::Weak<dyn std::any::Any + Send + Sync> {
        let strong: Arc<dyn std::any::Any + Send + Sync> = self.inner.clone();
        Arc::downgrade(&strong)
    }
}

impl PartialEq for MaterialTextures {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for MaterialTextures {}

/// A contiguous index range drawn with one alpha mode and, for PBR, one set of texture maps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submesh {
    /// Index range into `MeshData::indices`.
    pub indices: std::ops::Range<u32>,
    /// Whether this range blends (and therefore draws in the sorted transparent queue).
    pub blend: bool,
    /// Whether any triangle is alpha-tested, so shadow casters must test alpha too.
    pub masked: bool,
    /// Texture maps applied by the PBR pass; `None` uses the baked vertex factors alone.
    pub textures: Option<MaterialTextures>,
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
        for triangle in data.indices.as_chunks::<3>().0 {
            let blends = triangle.iter().any(|index| data.vertices[*index as usize].blends());
            if blends {
                blended.extend_from_slice(triangle);
            } else {
                opaque.extend_from_slice(triangle);
            }
        }
        let opaque_count = opaque.len() as u32;
        let total = (opaque.len() + blended.len()) as u32;
        let masked = opaque.iter().any(|index| data.vertices[*index as usize].masks());
        let mut submeshes = Vec::new();
        if opaque_count > 0 {
            submeshes.push(Submesh { indices: 0..opaque_count, blend: false, masked, textures: None });
        }
        if total > opaque_count {
            submeshes.push(Submesh { indices: opaque_count..total, blend: true, masked: false, textures: None });
        }
        opaque.extend(blended);

        let has_vertex_alpha = data.vertices.iter().any(|vertex| vertex.color[3] < 1.0);
        Ok(Self::assemble(MeshData { vertices: data.vertices, indices: opaque }, submeshes, has_vertex_alpha))
    }

    /// Build an asset from a TypeScript `PbrMesh` document, resolving texture sources through `resolve`.
    ///
    /// Matches the TypeScript material pass: every triangle gets its own three vertices, authored normals are
    /// normalized (zero normals use the triangle's face normal), material factors multiply vertex colors and texture
    /// samples, and triangles group by material with blended materials last. Alpha is governed by the materials, so the
    /// asset reports no vertex alpha. Fails for misaligned channels, invalid indices or material references,
    /// out-of-range material values, or a texture source that does not resolve.
    pub fn from_pbr_mesh(
        mesh: &PbrMesh,
        mut resolve: impl FnMut(&str) -> Option<TextureAsset>,
    ) -> Result<Self, RenderError> {
        let invalid = |message: &str| RenderError::InvalidMesh(message.to_owned());
        let count = mesh.positions.len() / 3;
        let aligned = mesh.positions.len().is_multiple_of(3)
            && mesh.normals.len() == count * 3
            && mesh.colors.len() == count * 3
            && mesh.uvs.len() == count * 2
            && mesh.alphas.len() == count
            && mesh.triangles.len().is_multiple_of(3)
            && mesh.triangle_materials.len() == mesh.triangles.len() / 3;
        if !aligned {
            return Err(invalid(
                "PBR geometry requires aligned positions, normals, colors, alpha, UVs and triangle materials.",
            ));
        }
        let channels = [&mesh.positions, &mesh.normals, &mesh.colors, &mesh.uvs, &mesh.alphas];
        if channels.iter().any(|values| values.iter().any(|value| !(*value as f32).is_finite())) {
            return Err(invalid("PBR vertex attributes must fit finite float32 values."));
        }
        let materials = mesh
            .materials
            .iter()
            .map(|material| resolve_material(material, &mut resolve))
            .collect::<Result<Vec<_>, _>>()?;

        // Triangle order within each material is preserved; materials draw in palette order, blended ones last.
        let mut order: Vec<usize> = (0..mesh.triangle_materials.len()).collect();
        for &material in &mesh.triangle_materials {
            if materials.get(material as usize).is_none() {
                return Err(invalid("PBR triangle references an unknown material."));
            }
        }
        order.sort_by_key(|&triangle| {
            let material = mesh.triangle_materials[triangle] as usize;
            (materials[material].blend, material)
        });

        let read = |values: &[f64], index: usize, width: usize| -> Vec<f32> {
            values[index * width..(index + 1) * width].iter().map(|value| *value as f32).collect()
        };
        let mut vertices = Vec::with_capacity(mesh.triangles.len());
        let mut submeshes: Vec<Submesh> = Vec::new();
        for triangle in order {
            let corners =
                [mesh.triangles[triangle * 3], mesh.triangles[triangle * 3 + 1], mesh.triangles[triangle * 3 + 2]];
            if corners.iter().any(|index| *index as usize >= count) {
                return Err(invalid("PBR triangle index is outside the vertex array."));
            }
            let material_index = mesh.triangle_materials[triangle] as usize;
            let material = &materials[material_index];
            let positions = corners.map(|index| Vec3::from_slice(&read(&mesh.positions, index as usize, 3)));
            let face = (positions[1] - positions[0]).cross(positions[2] - positions[0]);
            let flat = if face.length() > 0.0 { face.normalize() } else { Vec3::Y };
            let first = vertices.len() as u32;
            for (corner, index) in corners.iter().enumerate() {
                let index = *index as usize;
                let authored = Vec3::from_slice(&read(&mesh.normals, index, 3));
                let scale = authored.abs().max_element();
                let normal = if scale > 0.0 { (authored / scale).normalize() } else { flat };
                let color = read(&mesh.colors, index, 3);
                let alpha = mesh.alphas[index] as f32 * material.base[3];
                let rgba =
                    [color[0] * material.base[0], color[1] * material.base[1], color[2] * material.base[2], alpha];
                let uv = read(&mesh.uvs, index, 2);
                vertices.push(
                    MeshVertex::new(positions[corner].to_array(), normal.to_array(), rgba, material.factors)
                        .with_uv([uv[0], uv[1]]),
                );
            }
            match submeshes.last_mut() {
                Some(last)
                    if last.textures == material.textures
                        && last.blend == material.blend
                        && last.masked == material.masked =>
                {
                    last.indices.end = first + 3;
                }
                _ => submeshes.push(Submesh {
                    indices: first..first + 3,
                    blend: material.blend,
                    masked: material.masked,
                    textures: material.textures.clone(),
                }),
            }
        }
        if vertices.is_empty() {
            return Err(invalid("geometry needs at least one vertex and one triangle"));
        }
        let indices = (0..vertices.len() as u32).collect();
        Ok(Self::assemble(MeshData { vertices, indices }, submeshes, false))
    }

    fn assemble(data: MeshData, submeshes: Vec<Submesh>, has_vertex_alpha: bool) -> Self {
        let bounds = data.vertices.iter().fold(
            Bounds { min: Vec3::splat(f32::INFINITY), max: Vec3::splat(f32::NEG_INFINITY) },
            |bounds, vertex| {
                let position = Vec3::from_array(vertex.position);
                Bounds { min: bounds.min.min(position), max: bounds.max.max(position) }
            },
        );
        let id = NEXT_ASSET_ID.fetch_add(1, Ordering::Relaxed);
        Self { inner: Arc::new(MeshAssetInner { id, data, submeshes, bounds, has_vertex_alpha }) }
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

    /// Build an asset from the shared [`MeshAssetData`] record (the TypeScript `new MeshAsset(data)`), such as
    /// destructible chunk geometry, with default (fully rough, opaque) material factors.
    ///
    /// Positions, indices, normals, colors, and alphas convert as in [`MeshAsset::from_channels`]; UVs, materials,
    /// triangle materials, and metadata are not interpreted (FORM geometry with materials converts through
    /// `form_mesh_asset`). Fails for misaligned channels or invalid geometry.
    pub fn from_asset_data(data: &MeshAssetData) -> Result<Self, RenderError> {
        let narrow = |values: Option<&Vec<f64>>| -> Vec<f32> {
            values.map_or_else(Vec::new, |values| values.iter().map(|value| *value as f32).collect())
        };
        let positions = narrow(Some(&data.positions));
        let normals = narrow(data.normals.as_ref());
        let colors = narrow(data.colors.as_ref());
        let alphas = narrow(data.alphas.as_ref());
        Self::from_channels(MeshChannels {
            positions: &positions,
            indices: &data.indices,
            normals: &normals,
            colors: &colors,
            alphas: &alphas,
        })
    }

    /// Process-unique identity used to key GPU uploads.
    pub fn id(&self) -> u64 {
        self.inner.id
    }

    /// Validated geometry, with opaque triangles before blended ones.
    pub fn data(&self) -> &MeshData {
        &self.inner.data
    }

    /// Contiguous index ranges: opaque ones first, then blended ones.
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

/// A `PbrMaterial` converted to baked factors and shared texture maps.
struct ResolvedMaterial {
    base: [f32; 4],
    factors: VertexMaterial,
    blend: bool,
    masked: bool,
    textures: Option<MaterialTextures>,
}

/// Validate a material like the TypeScript `validateMaterial` and resolve its texture sources.
fn resolve_material(
    material: &PbrMaterial,
    resolve: &mut impl FnMut(&str) -> Option<TextureAsset>,
) -> Result<ResolvedMaterial, RenderError> {
    let invalid = |message: &str| RenderError::InvalidMesh(message.to_owned());
    let color_valid = |values: &[f64]| values.iter().all(|value| (*value as f32).is_finite() && *value >= 0.0);
    if !matches!(material.base_color.len(), 3 | 4)
        || material.emissive.len() != 3
        || !color_valid(&material.base_color)
        || !color_valid(&material.emissive)
    {
        return Err(invalid(
            "PBR material colors require nonnegative finite float32 channels (RGBA base color and RGB emissive).",
        ));
    }
    let alpha = material.base_color.get(3).copied().unwrap_or(1.0);
    if [alpha, material.metallic, material.roughness, material.alpha_cutoff]
        .iter()
        .any(|value| !(0.0..=1.0).contains(value))
    {
        return Err(invalid("PBR material alpha, metallic, roughness and alpha cutoff must be between 0 and 1."));
    }
    let mut texture = |source: &Option<String>| -> Result<Option<TextureAsset>, RenderError> {
        match source {
            None => Ok(None),
            Some(source) if source.trim().is_empty() => {
                Err(invalid("PBR material texture sources must be nonempty URLs or null."))
            }
            Some(source) => resolve(source)
                .map(Some)
                .ok_or_else(|| RenderError::InvalidMesh(format!("PBR texture \"{source}\" could not be loaded."))),
        }
    };
    let sources = &material.textures;
    let maps = TextureMaps {
        base_color: texture(&sources.base_color)?,
        normal: texture(&sources.normal)?,
        emissive: texture(&sources.emissive)?,
        metallic: texture(&sources.metallic)?,
        roughness: texture(&sources.roughness)?,
        ao: texture(&sources.ao)?,
    };
    let alpha_mode = match material.alpha_mode {
        MaterialAlphaMode::Opaque => AlphaMode::Opaque,
        MaterialAlphaMode::Mask => AlphaMode::Mask,
        MaterialAlphaMode::Blend => AlphaMode::Blend,
    };
    let color = |index: usize| material.base_color[index] as f32;
    Ok(ResolvedMaterial {
        base: [color(0), color(1), color(2), alpha as f32],
        factors: VertexMaterial {
            metallic: material.metallic as f32,
            roughness: material.roughness as f32,
            emissive: [material.emissive[0] as f32, material.emissive[1] as f32, material.emissive[2] as f32],
            alpha_mode,
            alpha_cutoff: material.alpha_cutoff as f32,
        },
        blend: alpha_mode == AlphaMode::Blend,
        masked: alpha_mode == AlphaMode::Mask,
        textures: (!maps.is_empty()).then(|| MaterialTextures::new(maps)),
    })
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
        assert_eq!(opaque.submeshes(), &[Submesh { indices: 0..3, blend: false, masked: false, textures: None }]);
        assert!(!opaque.has_vertex_alpha());

        let blended = triangle(0.5);
        assert_eq!(blended.submeshes(), &[Submesh { indices: 0..3, blend: true, masked: false, textures: None }]);
        assert!(blended.has_vertex_alpha());
        assert_ne!(opaque.id(), blended.id());
    }

    #[test]
    fn shared_mesh_records_convert_with_their_colors() {
        let data = MeshAssetData {
            positions: vec![0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 3.0, 0.0],
            indices: vec![0, 1, 2],
            colors: Some([1.0, 0.5, 0.25].repeat(3)),
            ..Default::default()
        };
        let asset = MeshAsset::from_asset_data(&data).unwrap();
        assert_eq!(asset.data().vertices[1].color, [1.0, 0.5, 0.25, 1.0]);
        assert_eq!(asset.bounds().max, Vec3::new(2.0, 3.0, 0.0));
        assert!(!asset.has_vertex_alpha());
        let misaligned = MeshAssetData { colors: Some(vec![1.0; 4]), ..data };
        assert!(MeshAsset::from_asset_data(&misaligned).is_err());
    }

    fn pbr_material(alpha_mode: MaterialAlphaMode, base_color: Option<&str>) -> PbrMaterial {
        PbrMaterial {
            base_color: vec![0.5, 1.0, 1.0, 0.8],
            emissive: vec![0.1, 0.0, 0.0],
            metallic: 0.25,
            roughness: 0.75,
            alpha_mode,
            alpha_cutoff: 0.4,
            textures: crate::core::PbrMaterialTextures {
                base_color: base_color.map(str::to_owned),
                ..Default::default()
            },
        }
    }

    fn pbr_mesh(materials: Vec<PbrMaterial>, triangle_materials: Vec<u32>) -> PbrMesh {
        PbrMesh {
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0],
            normals: vec![0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0],
            uvs: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            colors: vec![1.0; 12],
            alphas: vec![1.0, 1.0, 1.0, 0.5],
            triangles: vec![0, 2, 1, 1, 2, 3],
            triangle_materials,
            materials,
        }
    }

    #[test]
    fn pbr_meshes_group_materials_and_bake_factors() {
        let texture = TextureAsset::from_rgba8(1, 1, vec![255; 4]).unwrap();
        let materials = vec![
            pbr_material(MaterialAlphaMode::Blend, None),
            pbr_material(MaterialAlphaMode::Mask, Some("cutout.png")),
        ];
        let asset = MeshAsset::from_pbr_mesh(&pbr_mesh(materials, vec![0, 1]), |source| {
            (source == "cutout.png").then(|| texture.clone())
        })
        .unwrap();

        // Blended materials draw last; every triangle owns its three vertices.
        let submeshes = asset.submeshes();
        assert_eq!(submeshes.len(), 2);
        assert!(!submeshes[0].blend && submeshes[0].masked && submeshes[0].textures.is_some());
        assert!(submeshes[1].blend && !submeshes[1].masked && submeshes[1].textures.is_none());
        assert_eq!(asset.data().vertices.len(), 6);
        assert!(!asset.has_vertex_alpha(), "alpha belongs to the materials");

        // The masked triangle (1, 2, 3) comes first: factors multiply vertex color and alpha.
        let vertices = &asset.data().vertices;
        assert_eq!(vertices[0].position, [1.0, 0.0, 0.0]);
        assert_eq!(vertices[0].color, [0.5, 1.0, 1.0, 0.8]);
        assert_eq!(vertices[2].color[3], 0.5 * 0.8);
        assert_eq!(vertices[0].surface, [0.25, 0.75, 0.4, 1.0]);
        assert_eq!(vertices[0].emissive, [0.1, 0.0, 0.0, 0.0]);
        assert_eq!(vertices[2].uv, [1.0, 1.0]);
        // A zero authored normal takes the face normal; authored normals are normalized.
        assert_eq!(vertices[0].normal, [0.0, 1.0, 0.0]);
        assert_eq!(vertices[3].normal, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn pbr_meshes_reject_bad_documents() {
        let resolve = |_: &str| None;
        let missing = pbr_mesh(vec![pbr_material(MaterialAlphaMode::Opaque, Some("missing.png"))], vec![0, 0]);
        assert!(matches!(MeshAsset::from_pbr_mesh(&missing, resolve), Err(RenderError::InvalidMesh(_))));
        let unknown = pbr_mesh(vec![pbr_material(MaterialAlphaMode::Opaque, None)], vec![0, 3]);
        assert!(MeshAsset::from_pbr_mesh(&unknown, resolve).is_err());
        let mut misaligned = pbr_mesh(vec![pbr_material(MaterialAlphaMode::Opaque, None)], vec![0, 0]);
        misaligned.uvs.pop();
        assert!(MeshAsset::from_pbr_mesh(&misaligned, resolve).is_err());
        let mut rough = pbr_mesh(vec![pbr_material(MaterialAlphaMode::Opaque, None)], vec![0, 0]);
        rough.materials[0].roughness = 1.5;
        assert!(MeshAsset::from_pbr_mesh(&rough, resolve).is_err());
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
