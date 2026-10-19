// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM geometry as renderable mesh assets.
//!
//! FORM itself is renderer-free; this adapter is the only path from FORM output to the screen, so FORM LAB, games, and
//! examples all see the same result. Lit assets keep FORM's vertex colors. PBR assets multiply each vertex color by
//! its triangle's material base color and bake the material's metallic, roughness, emission, and alpha mode into the
//! vertices. Materials with
//! embedded maps (PNG, JPEG, or WebP data URLs) also keep FORM's texture coordinates and draw their decoded maps as
//! [`MaterialTextures`](joy_renderer::MaterialTextures).

use std::collections::HashMap;

use joy_core::{
    LinearColor,
    MaterialAlphaMode,
    PbrMaterial,
    PbrMaterialTextures,
    PbrMesh,
};
use joy_form::{
    FormMaterial,
    GeometryData,
};

use joy_renderer::{
    AlphaMode,
    MeshAsset,
    MeshData,
    MeshVertex,
    RenderError,
    Shading,
    TextureAsset,
    VertexMaterial,
};

/// Failure compiling FORM source into a mesh asset: the script's own diagnostic, or a rejected document or mesh.
#[derive(Debug, thiserror::Error)]
pub enum FormMeshError {
    /// FORM source failed to parse or evaluate; the diagnostic carries the source location.
    #[error("FORM evaluation failed: {0}")]
    Form(joy_form::Diagnostic),
    /// A FORM LAB document was malformed, or the evaluated geometry could not become a mesh asset.
    #[error(transparent)]
    Render(#[from] RenderError),
}

/// Sphere segments used for level primitives, matching the TypeScript `LevelScene` sphere asset.
const PRIMITIVE_SPHERE_SEGMENTS: u32 = 16;

/// Compile FORM source and convert the result for `shading`; the TypeScript `createFormMeshAsset` equivalent.
///
/// `overrides` is a JSON object of parameter values and `materials` an optional palette, both as accepted by
/// [`joy_form::CompiledScript::evaluate`].
pub fn compile_form_mesh(
    source: &str,
    overrides: &serde_json::Value,
    materials: Option<&serde_json::Value>,
    shading: Shading,
) -> Result<MeshAsset, FormMeshError> {
    let geometry = joy_form::compile(source, overrides, materials).map_err(FormMeshError::Form)?;
    Ok(form_mesh_asset(&geometry, shading)?)
}

/// Compiled FORM assets for levels and tools, cached by path, source text, parameters, and shading; the mesh half of the
/// TypeScript `AssetScene`.
///
/// `.form` files are FORM source; `.formlab` files are FORM LAB documents (`{"version": 1, "source", "overrides",
/// "materials"}`) whose overrides the component's parameters extend. Callers read files themselves (from disk, a
/// bundle, or `include_str!`), so the library never touches the file system and works the same in browsers.
#[derive(Default)]
pub struct FormAssetLibrary {
    cache: HashMap<String, MeshAsset>,
    textures: FormTextureCache,
}

impl FormAssetLibrary {
    /// An empty library.
    pub fn new() -> Self {
        Self::default()
    }

    /// Compile `text` (the contents of `path`) with `parameters`, reusing a cached asset when nothing changed.
    pub fn resolve(
        &mut self,
        path: &str,
        text: &str,
        parameters: &serde_json::Map<String, serde_json::Value>,
        shading: Shading,
    ) -> Result<MeshAsset, FormMeshError> {
        let key = serde_json::json!([path, text, parameters, format!("{shading:?}")]).to_string();
        if let Some(asset) = self.cache.get(&key) {
            return Ok(asset.clone());
        }
        let (source, mut overrides, materials) = if path.ends_with(".formlab") {
            let document: serde_json::Value = serde_json::from_str(text)
                .map_err(|error| RenderError::InvalidMesh(format!("{path}: FORM LAB document is not JSON: {error}")))?;
            if document.get("version").and_then(serde_json::Value::as_u64) != Some(1) {
                return Err(RenderError::InvalidMesh(format!("{path}: unsupported FORM LAB document version")).into());
            }
            let Some(source) = document.get("source").and_then(serde_json::Value::as_str) else {
                return Err(
                    RenderError::InvalidMesh(format!("{path}: FORM LAB document requires script source")).into()
                );
            };
            let overrides = match document.get("overrides") {
                Some(serde_json::Value::Object(overrides)) => overrides.clone(),
                _ => serde_json::Map::new(),
            };
            (source.to_owned(), overrides, document.get("materials").cloned())
        } else {
            (text.to_owned(), serde_json::Map::new(), None)
        };
        overrides.extend(parameters.iter().map(|(name, value)| (name.clone(), value.clone())));
        let geometry = joy_form::compile(&source, &serde_json::Value::Object(overrides), materials.as_ref())
            .map_err(FormMeshError::Form)?;
        let asset = form_mesh_asset_with_textures(&geometry, shading, &mut self.textures)?;
        self.cache.insert(key, asset.clone());
        Ok(asset)
    }

    /// Drop every cached asset and decoded texture; instances that still draw them keep their geometry alive.
    pub fn clear(&mut self) {
        self.cache.clear();
        self.textures.clear();
    }

    /// Keep only cached assets for which `keep` returns true, so a presentation can prune entries it no longer uses.
    pub fn retain(&mut self, mut keep: impl FnMut(&MeshAsset) -> bool) {
        self.cache.retain(|_, asset| keep(asset));
    }
}

/// Decoded material maps keyed by their embedded source, so rebuilding a FORM asset with unchanged materials reuses
/// the decoded images (and their GPU uploads) instead of decoding megabytes of base64 again.
#[derive(Clone, Debug, Default)]
pub struct FormTextureCache {
    textures: HashMap<String, TextureAsset>,
}

impl FormTextureCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// The decoded image for an embedded `data:image/…;base64,` source, decoding it on first use.
    pub fn resolve(&mut self, source: &str) -> Result<TextureAsset, RenderError> {
        if let Some(texture) = self.textures.get(source) {
            return Ok(texture.clone());
        }
        let texture = TextureAsset::decode_data_url(source)?;
        self.textures.insert(source.to_owned(), texture.clone());
        Ok(texture)
    }

    /// Keep only the images whose sources `keep` accepts, for example the sources of the current palette.
    pub fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.textures.retain(|source, _| keep(source));
    }

    /// Drop every decoded image; assets that use them keep them alive.
    pub fn clear(&mut self) {
        self.textures.clear();
    }

    /// Number of decoded images held.
    pub fn len(&self) -> usize {
        self.textures.len()
    }

    /// Whether no decoded image is held.
    pub fn is_empty(&self) -> bool {
        self.textures.is_empty()
    }
}

/// Convert evaluated FORM geometry into a mesh asset for `shading`, decoding any material maps.
///
/// Triangles are de-indexed only as far as materials require: a vertex shared by triangles with different materials is
/// duplicated per material, so per-vertex material factors stay exact. PBR geometry whose palette carries maps instead
/// gives every triangle its own vertices with texture coordinates, like the TypeScript material pass. Fails for empty or
/// malformed geometry and for maps that do not decode. Use [`form_mesh_asset_with_textures`] to reuse decoded maps
/// across rebuilds.
pub fn form_mesh_asset(geometry: &GeometryData, shading: Shading) -> Result<MeshAsset, RenderError> {
    form_mesh_asset_with_textures(geometry, shading, &mut FormTextureCache::new())
}

/// [`form_mesh_asset`] with decoded maps taken from and added to `textures`.
pub fn form_mesh_asset_with_textures(
    geometry: &GeometryData,
    shading: Shading,
    textures: &mut FormTextureCache,
) -> Result<MeshAsset, RenderError> {
    let vertex_count = geometry.positions.len() / 3;
    if geometry.triangles.is_empty() || vertex_count == 0 {
        return Err(RenderError::InvalidMesh("FORM geometry has no triangles".into()));
    }
    let materials = geometry.form_materials();
    let bake_materials = shading == Shading::Pbr;
    if bake_materials && materials.iter().any(has_textures) {
        return textured_form_mesh_asset(geometry, &materials, textures);
    }

    // Map (vertex, material) pairs to output vertices so shared vertices stay shared within a material.
    let mut remap = std::collections::HashMap::new();
    let mut vertices = Vec::with_capacity(vertex_count);
    let mut indices = Vec::with_capacity(geometry.triangles.len());
    for (triangle, corners) in geometry.triangles.as_chunks::<3>().0.iter().enumerate() {
        let material_index =
            if bake_materials { geometry.triangle_materials.get(triangle).copied().unwrap_or(0) } else { 0 };
        for &corner in corners {
            let key = (corner, material_index);
            let index = *remap.entry(key).or_insert_with(|| {
                let material = bake_materials.then(|| materials.get(material_index as usize)).flatten();
                vertices.push(form_vertex(geometry, corner as usize, material));
                (vertices.len() - 1) as u32
            });
            indices.push(index);
        }
    }
    MeshAsset::new(MeshData { vertices, indices })
}

/// Whether a material embeds any map.
fn has_textures(material: &FormMaterial) -> bool {
    let maps = &material.textures;
    [&maps.base_color, &maps.normal, &maps.emissive, &maps.metallic, &maps.roughness, &maps.ao]
        .iter()
        .any(|source| source.is_some())
}

/// Convert textured PBR geometry through the shared `PbrMesh` path, resolving maps through `textures`.
fn textured_form_mesh_asset(
    geometry: &GeometryData,
    materials: &[FormMaterial],
    textures: &mut FormTextureCache,
) -> Result<MeshAsset, RenderError> {
    let vertex_count = geometry.positions.len() / 3;
    let uvs = if geometry.uvs.len() == vertex_count * 2 { geometry.uvs.clone() } else { vec![0.0; vertex_count * 2] };
    let normals =
        if geometry.normals.len() == vertex_count * 3 { geometry.normals.clone() } else { vec![0.0; vertex_count * 3] };
    let mesh = PbrMesh {
        positions: geometry.positions.clone(),
        normals,
        uvs,
        colors: geometry.colors.clone(),
        alphas: geometry.alphas.clone(),
        triangles: geometry.triangles.clone(),
        triangle_materials: geometry.triangle_materials.clone(),
        materials: materials.iter().map(pbr_material).collect(),
    };
    let mut failure = None;
    let asset = MeshAsset::from_pbr_mesh(&mesh, |source| match textures.resolve(source) {
        Ok(texture) => Some(texture),
        Err(error) => {
            failure.get_or_insert(error);
            None
        }
    });
    match (asset, failure) {
        (Ok(asset), _) => Ok(asset),
        (Err(_), Some(error)) => Err(error),
        (Err(error), None) => Err(error),
    }
}

/// The renderer's material record for a typed FORM material.
fn pbr_material(material: &FormMaterial) -> PbrMaterial {
    let maps = &material.textures;
    PbrMaterial {
        base_color: material.base_color.to_vec(),
        emissive: material.emissive.to_vec(),
        metallic: material.metallic,
        roughness: material.roughness,
        alpha_mode: match material.alpha_mode {
            joy_form::AlphaMode::Opaque => MaterialAlphaMode::Opaque,
            joy_form::AlphaMode::Mask => MaterialAlphaMode::Mask,
            joy_form::AlphaMode::Blend => MaterialAlphaMode::Blend,
        },
        alpha_cutoff: material.alpha_cutoff,
        textures: PbrMaterialTextures {
            base_color: maps.base_color.clone(),
            normal: maps.normal.clone(),
            emissive: maps.emissive.clone(),
            metallic: maps.metallic.clone(),
            roughness: maps.roughness.clone(),
            ao: maps.ao.clone(),
        },
    }
}

/// Unit box (full extents of one) with white vertices; instances supply size and color. Matches `meshBox(1, 1, 1)`.
pub fn primitive_box() -> MeshAsset {
    let geometry = joy_form::mesh_box(1.0, 1.0, 1.0).expect("unit box parameters are valid");
    primitive_asset(&geometry)
}

/// Sphere of radius one half with white vertices; instances supply size and color. Matches `meshSphere(0.5, 16)`.
pub fn primitive_sphere() -> MeshAsset {
    let geometry =
        joy_form::mesh_sphere(0.5, PRIMITIVE_SPHERE_SEGMENTS).expect("primitive sphere parameters are valid");
    primitive_asset(&geometry)
}

/// Build a white, default-material asset from primitive geometry (the TypeScript primitives carry no colors).
fn primitive_asset(geometry: &GeometryData) -> MeshAsset {
    let vertices = (0..geometry.positions.len() / 3)
        .map(|index| {
            let mut vertex = form_vertex(geometry, index, None);
            vertex.color = [1.0; 4];
            vertex
        })
        .collect();
    MeshAsset::new(MeshData { vertices, indices: geometry.triangles.clone() })
        .expect("FORM primitives are valid geometry")
}

/// Convert one FORM vertex, optionally baking a material.
fn form_vertex(geometry: &GeometryData, index: usize, material: Option<&FormMaterial>) -> MeshVertex {
    let triple = |values: &[f64], fallback: f64| -> [f32; 3] {
        let start = index * 3;
        match values.get(start..start + 3) {
            Some(slice) => [slice[0] as f32, slice[1] as f32, slice[2] as f32],
            None => [fallback as f32; 3],
        }
    };
    let position = triple(&geometry.positions, 0.0);
    let normal = triple(&geometry.normals, 0.0);
    let color = triple(&geometry.colors, 1.0);
    let alpha = geometry.alphas.get(index).copied().unwrap_or(1.0) as f32;

    let Some(material) = material else {
        // Lit and Unlit assets ignore materials; vertex alpha alone selects transparency, as in TypeScript.
        let vertex_material = VertexMaterial {
            alpha_mode: if alpha < 1.0 { AlphaMode::Blend } else { AlphaMode::Opaque },
            ..VertexMaterial::default()
        };
        return MeshVertex::new(
            position,
            normal,
            LinearColor::from_rgb_array(color).with_alpha(alpha),
            vertex_material,
        );
    };

    let base = material.base_color;
    let baked_color = LinearColor::new(
        color[0] * base[0] as f32,
        color[1] * base[1] as f32,
        color[2] * base[2] as f32,
        alpha * base[3] as f32,
    );
    let vertex_material = VertexMaterial {
        metallic: material.metallic as f32,
        roughness: material.roughness as f32,
        emissive: LinearColor::rgb(
            material.emissive[0] as f32,
            material.emissive[1] as f32,
            material.emissive[2] as f32,
        ),
        alpha_mode: match material.alpha_mode {
            joy_form::AlphaMode::Opaque => AlphaMode::Opaque,
            joy_form::AlphaMode::Mask => AlphaMode::Mask,
            joy_form::AlphaMode::Blend => AlphaMode::Blend,
        },
        alpha_cutoff: material.alpha_cutoff as f32,
    };
    MeshVertex::new(position, normal, baked_color, vertex_material)
}

#[cfg(test)]
#[path = "form_assets.test.rs"]
mod tests;
