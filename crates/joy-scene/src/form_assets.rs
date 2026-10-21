// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM geometry as renderable mesh assets.
//!
//! FORM itself is renderer-free; this adapter is the only path from FORM output to the screen, so FORM LAB, games, and
//! examples all see the same result. Evaluated geometry becomes an engine [`Mesh`](joy_render_types::Mesh) whose
//! material slots follow FORM's palette, and each palette entry becomes a material drawn with the shading model the
//! caller picks: [`ShadingModel::Layered`] keeps every factor and embedded map (PNG, JPEG, or WebP data URLs), while
//! [`ShadingModel::Lit`] and [`ShadingModel::Unlit`] draw FORM's vertex colors alone in one slot, as FORM's flat look
//! always has.

use std::collections::HashMap;
use std::sync::Arc;

use joy_form::GeometryData;
use joy_render_types::ShadingModel;

use joy_renderer::{
    MaterialAsset,
    MeshAsset,
    RenderError,
    TextureAsset,
};

use crate::form_placements::{
    FormPlacement,
    form_placements,
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

/// Sphere segments used for level primitives.
const PRIMITIVE_SPHERE_SEGMENTS: u32 = 16;

/// Compile FORM source and convert the result, its palette drawn with `model`.
///
/// `overrides` is a JSON object of parameter values and `materials` an optional palette, both as accepted by
/// [`joy_form::CompiledScript::evaluate`].
pub fn compile_form_mesh(
    source: &str,
    overrides: &serde_json::Value,
    materials: Option<&serde_json::Value>,
    model: ShadingModel<'_>,
) -> Result<MeshAsset, FormMeshError> {
    let geometry = joy_form::compile(source, overrides, materials).map_err(FormMeshError::Form)?;
    Ok(form_mesh_asset(&geometry, model)?)
}

/// A compiled FORM asset: the mesh it draws and the placements its tagged points ask the host to fill.
#[derive(Clone, Debug)]
pub struct FormAsset {
    /// The mesh drawn for the asset's visible geometry.
    pub mesh: MeshAsset,
    /// Tagged points in the asset's local space; see [`form_placements`].
    pub placements: Arc<[FormPlacement]>,
}

/// Compiled FORM assets for levels and tools, cached by path, source text, variant, parameters, and shading model; the
/// mesh half of [`AssetScene`](crate::AssetScene).
///
/// `.form` files are FORM source; `.formlab` files are FORM LAB documents (`{"version": 1, "source", "overrides",
/// "materials", "variants"}`). Overrides layer in order: the document's overrides, then the named variant's, then the
/// component's parameters. Callers read files themselves (from disk, a bundle, or `include_str!`), so the library never
/// touches the file system and works the same in browsers.
#[derive(Default)]
pub struct FormAssetLibrary {
    cache: HashMap<String, FormAsset>,
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
        model: ShadingModel<'_>,
    ) -> Result<MeshAsset, FormMeshError> {
        Ok(self.resolve_asset(path, text, None, parameters, model)?.mesh)
    }

    /// Compile `text` (the contents of `path`) as its mesh and placements, layering the `.formlab` document's overrides,
    /// then `variant`'s overrides (a FORM LAB document's named variant; fails when the document has none by that name),
    /// then `parameters`. Reuses a cached asset when nothing changed.
    pub fn resolve_asset(
        &mut self,
        path: &str,
        text: &str,
        variant: Option<&str>,
        parameters: &serde_json::Map<String, serde_json::Value>,
        model: ShadingModel<'_>,
    ) -> Result<FormAsset, FormMeshError> {
        let key = serde_json::json!([path, text, variant, parameters, model.name()]).to_string();
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
            let mut overrides = match document.get("overrides") {
                Some(serde_json::Value::Object(overrides)) => overrides.clone(),
                _ => serde_json::Map::new(),
            };
            if let Some(name) = variant {
                overrides.extend(variant_overrides(&document, name).ok_or_else(|| {
                    RenderError::InvalidMesh(format!("{path}: no FORM LAB variant named \"{name}\""))
                })?);
            }
            (source.to_owned(), overrides, document.get("materials").cloned())
        } else {
            if let Some(name) = variant {
                return Err(RenderError::InvalidMesh(format!(
                    "{path}: variant \"{name}\" requires a FORM LAB document"
                ))
                .into());
            }
            (text.to_owned(), serde_json::Map::new(), None)
        };
        overrides.extend(parameters.iter().map(|(name, value)| (name.clone(), value.clone())));
        let geometry = joy_form::compile(&source, &serde_json::Value::Object(overrides), materials.as_ref())
            .map_err(FormMeshError::Form)?;
        let asset = FormAsset {
            mesh: form_mesh_asset_with_textures(&geometry, model, &mut self.textures)?,
            placements: form_placements(&geometry).into(),
        };
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
        self.cache.retain(|_, asset| keep(&asset.mesh));
    }
}

/// The overrides of the variant named `name` (ignoring case, as FORM LAB keeps names unique) in a FORM LAB document.
fn variant_overrides(document: &serde_json::Value, name: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    let variants = document.get("variants")?.as_array()?;
    let variant = variants.iter().find(|variant| {
        variant
            .get("name")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
    })?;
    match variant.get("overrides") {
        Some(serde_json::Value::Object(overrides)) => Some(overrides.clone()),
        _ => Some(serde_json::Map::new()),
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

/// Convert evaluated FORM geometry into a mesh asset whose palette draws with `model`, decoding any material maps.
///
/// Fails for empty or malformed geometry, for maps that do not decode, and for a custom model (FORM palettes describe
/// only the built-in models). Use [`form_mesh_asset_with_textures`] to reuse decoded maps across rebuilds.
pub fn form_mesh_asset(geometry: &GeometryData, model: ShadingModel<'_>) -> Result<MeshAsset, RenderError> {
    form_mesh_asset_with_textures(geometry, model, &mut FormTextureCache::new())
}

/// [`form_mesh_asset`] with decoded maps taken from and added to `textures`.
pub fn form_mesh_asset_with_textures(
    geometry: &GeometryData,
    model: ShadingModel<'_>,
    textures: &mut FormTextureCache,
) -> Result<MeshAsset, RenderError> {
    let mut mesh = geometry.to_mesh();
    let palette = geometry.form_materials();

    // FORM's flat looks ignore the palette, so every triangle shares one slot and one material.
    let palette = if model == ShadingModel::Layered {
        palette
    } else {
        mesh.triangle_slots = None;
        mesh.material_slots.clear();
        palette.into_iter().take(1).collect()
    };

    let mut materials = Vec::with_capacity(palette.len());
    for entry in &palette {
        let Some(material) = entry.to_material(model) else {
            return Err(RenderError::InvalidMaterial(format!(
                "FORM palettes convert only to built-in shading models, not \"{}\"",
                model.name()
            )));
        };
        let mut failure = None;
        let asset = MaterialAsset::with_textures(material, |source| match textures.resolve(source) {
            Ok(texture) => Some(texture),
            Err(error) => {
                failure.get_or_insert(error);
                None
            }
        });
        // A map that failed to decode reports its decode error rather than a generic unresolved source.
        match (asset, failure) {
            (Ok(asset), _) => materials.push(asset),
            (Err(_), Some(error)) => return Err(error),
            (Err(error), None) => return Err(error),
        }
    }
    MeshAsset::with_materials(&mesh, materials)
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

/// Build a white asset with the default material from primitive geometry; instances supply the color.
fn primitive_asset(geometry: &GeometryData) -> MeshAsset {
    let mut mesh = geometry.to_mesh();
    mesh.colors = None;
    mesh.triangle_slots = None;
    mesh.material_slots.clear();
    MeshAsset::new(&mesh).expect("FORM primitives are valid geometry")
}

#[cfg(test)]
#[path = "form_assets.test.rs"]
mod tests;
