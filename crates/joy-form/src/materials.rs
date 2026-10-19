// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM material palettes: serialized validation and the typed [`FormMaterial`] view renderers consume.

use serde::{
    Deserialize,
    Serialize,
};
use serde_json::{
    Value as Json,
    json,
};
use std::collections::HashSet;

const MAX_MATERIALS: usize = 128;
pub(crate) const MAX_TEXTURE_BYTES: usize = 4_000_000;
pub(crate) const MAX_TOTAL_TEXTURE_BYTES: usize = 32_000_000;
const MATERIAL_FIELDS: &[&str] =
    &["name", "baseColor", "metallic", "roughness", "emissive", "alphaMode", "alphaCutoff", "textures"];
const TEXTURE_FIELDS: &[&str] = &["baseColor", "normal", "emissive", "metallic", "roughness", "ao"];

/// Name of the palette entry FORM always keeps first.
pub(crate) const DEFAULT_MATERIAL_NAME: &str = "Default";
const DEFAULT_BASE_COLOR: [f64; 4] = [1.0, 1.0, 1.0, 1.0];
const DEFAULT_METALLIC: f64 = 0.0;
const DEFAULT_ROUGHNESS: f64 = 1.0;
const DEFAULT_EMISSIVE: [f64; 3] = [0.0, 0.0, 0.0];
const DEFAULT_ALPHA_CUTOFF: f64 = 0.5;

/// How a renderer combines a material's alpha with the framebuffer, following glTF.
///
/// Serialized as `"OPAQUE"`, `"MASK"`, or `"BLEND"`, the glTF `alphaMode` spelling.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[serde(rename_all = "UPPERCASE")]
pub enum AlphaMode {
    /// Alpha is ignored; the surface is fully opaque.
    #[default]
    Opaque,
    /// Fragments with alpha below [`FormMaterial::alpha_cutoff`] are discarded.
    Mask,
    /// Alpha blends the surface over what is behind it.
    Blend,
}

/// Embedded texture sources for one material, each a `data:image/(png|jpeg|webp);base64,` URI.
///
/// `None` means the slot has no texture. Validated palettes limit each texture to 4 MB of image
/// data and a palette to 32 MB in total.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FormMaterialTextures {
    /// Base color (albedo) image.
    pub base_color: Option<String>,
    /// Tangent-space normal map.
    pub normal: Option<String>,
    /// Emissive color image.
    pub emissive: Option<String>,
    /// Metallic factor image.
    pub metallic: Option<String>,
    /// Roughness factor image.
    pub roughness: Option<String>,
    /// Ambient occlusion image.
    pub ao: Option<String>,
}

/// A typed FORM PBR material.
///
/// Factors are linear: the base color is RGBA in 0–1 (it is a reflectance), emission is
/// nonnegative RGB that may exceed one for HDR glow, and `metallic`, `roughness`, and
/// `alpha_cutoff` are normalized factors in 0–1. The serialized form uses camelCase field names,
/// so `serde_json::to_value` produces a record FORM palettes accept. [`Default`] is FORM's default
/// material. Values are owned copies; editing one does not affect its geometry.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FormMaterial {
    /// Unique palette name: an ASCII identifier of at most 64 characters.
    pub name: String,
    /// Linear RGBA base color factor.
    pub base_color: [f64; 4],
    /// Metalness factor.
    pub metallic: f64,
    /// Perceptual roughness factor.
    pub roughness: f64,
    /// Linear RGB emissive factor; nonnegative, and above one for HDR glow.
    pub emissive: [f64; 3],
    /// Alpha handling for this material.
    pub alpha_mode: AlphaMode,
    /// Alpha threshold used by [`AlphaMode::Mask`].
    pub alpha_cutoff: f64,
    /// Optional embedded textures.
    pub textures: FormMaterialTextures,
}

impl Default for FormMaterial {
    fn default() -> Self {
        Self {
            name: DEFAULT_MATERIAL_NAME.to_owned(),
            base_color: DEFAULT_BASE_COLOR,
            metallic: DEFAULT_METALLIC,
            roughness: DEFAULT_ROUGHNESS,
            emissive: DEFAULT_EMISSIVE,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: DEFAULT_ALPHA_CUTOFF,
            textures: FormMaterialTextures::default(),
        }
    }
}

impl FormMaterial {
    /// Convert a serialized material record without failing.
    ///
    /// Each field that is missing or invalid (a non-string name, a tuple of the wrong length,
    /// a factor outside its range, or an unknown alpha mode) takes its `FormMaterial::default()` value, and
    /// a texture slot that is not a string is `None`. Records produced by evaluation are already
    /// validated and convert exactly. The input is only borrowed.
    pub fn from_json(value: &Json) -> Self {
        let defaults = Self::default();
        let textures = &value["textures"];
        let texture = |slot: &str| textures[slot].as_str().map(str::to_owned);
        Self {
            name: value["name"].as_str().map_or(defaults.name, str::to_owned),
            base_color: unit_tuple(&value["baseColor"]).unwrap_or(defaults.base_color),
            metallic: unit_factor(&value["metallic"]).unwrap_or(defaults.metallic),
            roughness: unit_factor(&value["roughness"]).unwrap_or(defaults.roughness),
            emissive: emissive_tuple(&value["emissive"]).unwrap_or(defaults.emissive),
            alpha_mode: AlphaMode::deserialize(&value["alphaMode"]).unwrap_or(defaults.alpha_mode),
            alpha_cutoff: unit_factor(&value["alphaCutoff"]).unwrap_or(defaults.alpha_cutoff),
            textures: FormMaterialTextures {
                base_color: texture("baseColor"),
                normal: texture("normal"),
                emissive: texture("emissive"),
                metallic: texture("metallic"),
                roughness: texture("roughness"),
                ao: texture("ao"),
            },
        }
    }
}

/// Return an owned copy of the editable FORM default material.
pub fn default_material() -> Json {
    json!({
        "name": DEFAULT_MATERIAL_NAME, "baseColor": [1, 1, 1, 1], "metallic": 0,
        "roughness": 1, "emissive": [0, 0, 0], "alphaMode": "OPAQUE",
        "alphaCutoff": 0.5, "textures": empty_textures()
    })
}

/// Validate borrowed records and return an owned palette with Default first.
pub fn read_materials(value: Option<&Json>) -> Result<Vec<Json>, String> {
    let Some(value) = value else {
        return Ok(vec![default_material()]);
    };
    let candidates = value.as_array().ok_or("Materials must be an array.")?;
    let mut materials = Vec::new();
    let mut names = HashSet::new();
    let mut texture_bytes = 0usize;
    for (index, candidate) in candidates.iter().enumerate() {
        let material = read_material(candidate, index)?;
        for field in TEXTURE_FIELDS {
            if let Some(source) = material["textures"][field].as_str() {
                texture_bytes += texture_byte_length(source);
            }
        }
        if texture_bytes > MAX_TOTAL_TEXTURE_BYTES {
            return Err("Combined material textures must be 32 MB or smaller.".into());
        }
        let name = material["name"].as_str().unwrap().to_owned();
        if !names.insert(name.clone()) {
            return Err(format!("Material names must be unique; '{name}' is duplicated."));
        }
        materials.push(material);
    }
    match materials.iter().position(|material| material["name"] == DEFAULT_MATERIAL_NAME) {
        None => materials.insert(0, default_material()),
        Some(0) => {}
        Some(index) => {
            let default = materials.remove(index);
            materials.insert(0, default);
        }
    }
    if materials.len() > MAX_MATERIALS {
        return Err(format!("At most {MAX_MATERIALS} materials are supported."));
    }
    Ok(materials)
}

fn read_material(value: &Json, index: usize) -> Result<Json, String> {
    let record = value.as_object().ok_or_else(|| format!("Material {index} must be an object."))?;
    for field in record.keys() {
        if !MATERIAL_FIELDS.contains(&field.as_str()) {
            let name = record.get("name").and_then(Json::as_str).map_or_else(|| index.to_string(), str::to_owned);
            return Err(format!("Material '{name}' has unsupported field '{field}'."));
        }
    }
    let name = record
        .get("name")
        .and_then(Json::as_str)
        .filter(|name| {
            let bytes = name.as_bytes();
            !bytes.is_empty()
                && bytes.len() <= 64
                && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
                && bytes[1..].iter().all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        })
        .ok_or_else(|| format!("Material {index} name must be an ASCII identifier of at most 64 characters."))?;
    let base_color = read_tuple(&value["baseColor"], 4, &format!("{name}.baseColor"))?;
    let emissive = read_emissive(&value["emissive"], &format!("{name}.emissive"))?;
    let metallic = read_unit_number(&value["metallic"], &format!("{name}.metallic"))?;
    let roughness = read_unit_number(&value["roughness"], &format!("{name}.roughness"))?;
    let alpha_cutoff = read_unit_number(&value["alphaCutoff"], &format!("{name}.alphaCutoff"))?;
    let textures = read_textures(record.get("textures"), name)?;
    if !value["alphaMode"].as_str().is_some_and(|mode| ["OPAQUE", "MASK", "BLEND"].contains(&mode)) {
        return Err(format!("{name}.alphaMode must be OPAQUE, MASK, or BLEND."));
    }
    Ok(json!({"name":name, "baseColor":base_color, "metallic":metallic,
        "roughness":roughness, "emissive":emissive, "alphaMode":value["alphaMode"],
        "alphaCutoff":alpha_cutoff, "textures":textures}))
}

fn empty_textures() -> Json {
    json!({"baseColor":null, "normal":null, "emissive":null, "metallic":null, "roughness":null, "ao":null})
}

fn read_textures(value: Option<&Json>, name: &str) -> Result<Json, String> {
    let mut result = empty_textures();
    let Some(value) = value else {
        return Ok(result);
    };
    let textures = value.as_object().ok_or_else(|| format!("{name}.textures must be an object."))?;
    for (key, source) in textures {
        if !TEXTURE_FIELDS.contains(&key.as_str()) || !(source.is_null() || source.is_string()) {
            return Err(format!("{name}.textures contains an invalid '{key}' texture."));
        }
        if let Some(source) = source.as_str() {
            if !["data:image/png;base64,", "data:image/jpeg;base64,", "data:image/webp;base64,"]
                .iter()
                .any(|prefix| source.starts_with(prefix))
            {
                return Err(format!("{name}.{key} must be an embedded PNG, JPEG, or WebP texture."));
            }
            if texture_byte_length(source) > MAX_TEXTURE_BYTES {
                return Err("Textures must be 4 MB or smaller.".into());
            }
            let payload = source.split_once(',').unwrap().1;
            let unpadded = payload.trim_end_matches('=');
            let padding = payload.len() - unpadded.len();
            if payload.is_empty()
                || payload.len() % 4 != 0
                || padding > 2
                || !unpadded.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/')
            {
                return Err(format!("{name}.{key} contains invalid base64 texture data."));
            }
        }
        result[key] = source.clone();
    }
    Ok(result)
}

fn texture_byte_length(source: &str) -> usize {
    let payload = source.split_once(',').map_or(source, |(_, payload)| payload);
    let padding = if source.ends_with("==") { 2 } else { usize::from(source.ends_with('=')) };
    (payload.len() * 3 / 4).saturating_sub(padding)
}

fn read_tuple(value: &Json, length: usize, label: &str) -> Result<Vec<f64>, String> {
    let values = value
        .as_array()
        .filter(|values| values.len() == length)
        .ok_or_else(|| format!("{label} must contain exactly {length} numbers."))?;
    values.iter().map(|component| read_unit_number(component, label)).collect()
}

/// Emission is linear light: finite and nonnegative, and above one for HDR glow.
fn read_emissive(value: &Json, label: &str) -> Result<Vec<f64>, String> {
    emissive_tuple(value)
        .map(|emissive| emissive.to_vec())
        .ok_or_else(|| format!("{label} must contain exactly 3 finite, nonnegative numbers."))
}

fn read_unit_number(value: &Json, label: &str) -> Result<f64, String> {
    value
        .as_f64()
        .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
        .ok_or_else(|| format!("{label} must be a finite number from 0 to 1."))
}

// Typed-view helpers accept exactly what the serialized validator accepts and report the rest
// as absent, so a malformed field falls back to its default instead of failing.
fn unit_factor(value: &Json) -> Option<f64> {
    value.as_f64().filter(|factor| factor.is_finite() && (0.0..=1.0).contains(factor))
}

fn emissive_tuple(value: &Json) -> Option<[f64; 3]> {
    let components = value.as_array().filter(|components| components.len() == 3)?;
    let mut tuple = [0.0; 3];
    for (slot, component) in tuple.iter_mut().zip(components) {
        *slot = component.as_f64().filter(|channel| channel.is_finite() && *channel >= 0.0)?;
    }
    Some(tuple)
}

fn unit_tuple<const LENGTH: usize>(value: &Json) -> Option<[f64; LENGTH]> {
    let components = value.as_array().filter(|components| components.len() == LENGTH)?;
    let mut tuple = [0.0; LENGTH];
    for (slot, component) in tuple.iter_mut().zip(components) {
        *slot = unit_factor(component)?;
    }
    Some(tuple)
}
