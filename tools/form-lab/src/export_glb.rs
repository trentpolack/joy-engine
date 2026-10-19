// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Self-contained glTF 2.0 binary (GLB) export of an accepted build with its authored materials and embedded maps.
//! Output matches reference files from the retired TypeScript exporter byte for byte.
//!
//! Triangles group into one primitive per material, in first-use order, with every triangle's three corners written
//! out so hard edges keep their own normals; points become `POINTS` primitives. Positions carry exact float32 bounds,
//! normals are unit length (authored where nonzero, otherwise the face normal), colors are RGBA, and UVs travel when
//! the geometry has one pair per vertex. Materials stay two-sided metallic-roughness materials; WebP maps declare the
//! required `EXT_texture_webp` extension without inventing a fallback.

use joy_engine::core::decode_base64;
use joy_engine::form::{
    GeometryData,
    read_materials,
};
use joy_engine::math::DVec3;
use joy_engine::serde_json::{
    Map,
    Value as Json,
    json,
};

use crate::export::integral_numbers_as_integers;

/// `glTF` in little-endian ASCII.
const GLB_MAGIC: u32 = 0x4654_6c67;
/// The JSON chunk type.
const JSON_CHUNK: u32 = 0x4e4f_534a;
/// The binary chunk type.
const BINARY_CHUNK: u32 = 0x004e_4942;
/// `ARRAY_BUFFER` buffer-view target.
const ARRAY_BUFFER: u32 = 34962;
/// `FLOAT` accessor component type.
const FLOAT: u32 = 5126;
/// Triangle primitive mode.
const TRIANGLES: u32 = 4;
/// Point primitive mode.
const POINTS: u32 = 0;

/// Why geometry could not become a GLB. Messages match the TypeScript exporter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlbExportError(pub String);

impl std::fmt::Display for GlbExportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for GlbExportError {}

/// Export authored triangle and point geometry as a self-contained glTF 2.0 binary (`exportGlb`).
///
/// Empty `normals` or `uvs` mean the geometry has none. Fails for empty or inconsistent geometry, values that do not
/// fit float32, colors outside 0–1, invalid indices or material assignments, degenerate triangles, a palette that does
/// not start with `Default`, and maps that are not PNG, JPEG, or WebP data URLs.
pub fn export_glb(data: &GeometryData) -> Result<Vec<u8>, GlbExportError> {
    validate_geometry(data)?;
    if data.materials.first().and_then(|material| material.get("name")).and_then(Json::as_str) != Some("Default") {
        return Err(error("GLB materials must contain 'Default' at index 0."));
    }
    let materials = read_materials(Some(&Json::Array(data.materials.clone()))).map_err(|failure| error(&failure.0))?;
    validate_assignments(data, materials.len())?;

    let mut writer = BinaryWriter::default();
    let normals = (!data.normals.is_empty()).then_some(data.normals.as_slice());
    let vertex_count = data.positions.len() / 3;
    let uvs = (data.uvs.len() == vertex_count * 2).then_some(data.uvs.as_slice());

    let mut primitives = Vec::new();
    for (material, triangle_indexes) in group_assignments(&data.triangle_materials) {
        let mut positions = Vec::new();
        let mut normal_values = Vec::new();
        let mut colors = Vec::new();
        let mut coordinates = Vec::new();
        for triangle in triangle_indexes {
            let corners = &data.triangles[triangle * 3..(triangle * 3) + 3];
            let face_normal = flat_normal(&data.positions, corners)?;
            for &corner in corners {
                let index = corner as usize;
                positions.extend_from_slice(&data.positions[index * 3..(index * 3) + 3]);
                let authored =
                    normals.map(|normals| [normals[index * 3], normals[(index * 3) + 1], normals[(index * 3) + 2]]);
                let normal = match authored {
                    Some(authored) if authored.iter().any(|value| *value != 0.0) => authored,
                    _ => face_normal,
                };
                let length = DVec3::from_array(normal).length();
                normal_values.extend(normal.iter().map(|value| value / length));
                colors.extend_from_slice(&data.colors[index * 3..(index * 3) + 3]);
                colors.push(data.alphas[index]);
                if let Some(uvs) = uvs {
                    coordinates.extend_from_slice(&uvs[index * 2..(index * 2) + 2]);
                }
            }
        }
        let mut attributes = Map::new();
        attributes.insert("POSITION".into(), json!(writer.accessor(&positions, 3, true)));
        attributes.insert("NORMAL".into(), json!(writer.accessor(&normal_values, 3, false)));
        attributes.insert("COLOR_0".into(), json!(writer.accessor(&colors, 4, false)));
        if uvs.is_some() {
            attributes.insert("TEXCOORD_0".into(), json!(writer.accessor(&coordinates, 2, false)));
        }
        primitives.push(json!({"attributes": attributes, "mode": TRIANGLES, "material": material}));
    }
    for (material, point_indexes) in group_assignments(&data.point_materials) {
        let mut positions = Vec::new();
        let mut colors = Vec::new();
        for assignment in point_indexes {
            let index = data.points[assignment] as usize;
            positions.extend_from_slice(&data.positions[index * 3..(index * 3) + 3]);
            colors.extend_from_slice(&data.colors[index * 3..(index * 3) + 3]);
            colors.push(data.alphas[index]);
        }
        let position = writer.accessor(&positions, 3, true);
        let color = writer.accessor(&colors, 4, false);
        primitives.push(
            json!({"attributes": {"POSITION": position, "COLOR_0": color}, "mode": POINTS, "material": material}),
        );
    }

    let mut textures = TextureWriter::default();
    let materials = materials
        .iter()
        .map(|material| export_material(material, &mut textures, &mut writer))
        .collect::<Result<Vec<_>, _>>()?;

    let uses_emissive_strength =
        materials.iter().any(|material| material["extensions"].get(EMISSIVE_STRENGTH_EXTENSION).is_some());

    let mut gltf = Map::new();
    gltf.insert("asset".into(), json!({"version": "2.0", "generator": "FORM LAB"}));
    gltf.insert("scene".into(), json!(0));
    gltf.insert("scenes".into(), json!([{"nodes": [0]}]));
    gltf.insert("nodes".into(), json!([{"mesh": 0, "name": "FormLab"}]));
    gltf.insert("meshes".into(), json!([{"name": "FormLab", "primitives": primitives}]));
    gltf.insert("materials".into(), Json::Array(materials));
    gltf.insert("buffers".into(), json!([{"byteLength": writer.length}]));
    gltf.insert("bufferViews".into(), Json::Array(writer.buffer_views));
    gltf.insert("accessors".into(), Json::Array(writer.accessors));
    if !textures.images.is_empty() {
        gltf.insert("images".into(), Json::Array(textures.images));
        gltf.insert("textures".into(), Json::Array(textures.textures));
    }
    let mut extensions_used = Vec::new();
    if textures.uses_webp {
        extensions_used.push(WEBP_EXTENSION);
    }
    if uses_emissive_strength {
        extensions_used.push(EMISSIVE_STRENGTH_EXTENSION);
    }
    if !extensions_used.is_empty() {
        gltf.insert("extensionsUsed".into(), json!(extensions_used));
    }
    // WebP has no core fallback, so files using it require the extension; emissive strength degrades gracefully.
    if textures.uses_webp {
        gltf.insert("extensionsRequired".into(), json!([WEBP_EXTENSION]));
    }
    Ok(write_glb(&integral_numbers_as_integers(Json::Object(gltf)).to_string(), &writer.bytes))
}

/// glTF extension for WebP textures.
const WEBP_EXTENSION: &str = "EXT_texture_webp";

/// glTF extension that scales `emissiveFactor` above one for HDR emission.
const EMISSIVE_STRENGTH_EXTENSION: &str = "KHR_materials_emissive_strength";

/// The exporter's error with a message.
fn error(message: &str) -> GlbExportError {
    GlbExportError(message.to_owned())
}

/// Accumulates the binary chunk, its buffer views, and accessors.
#[derive(Default)]
struct BinaryWriter {
    bytes: Vec<u8>,
    length: usize,
    buffer_views: Vec<Json>,
    accessors: Vec<Json>,
}

impl BinaryWriter {
    /// Append float32 values as a vertex attribute accessor and return its index.
    fn accessor(&mut self, values: &[f64], component_count: usize, include_bounds: bool) -> usize {
        let floats: Vec<f32> = values.iter().map(|value| *value as f32).collect();
        let bytes: Vec<u8> = floats.iter().flat_map(|value| value.to_le_bytes()).collect();
        let buffer_view = self.buffer_views.len();
        self.buffer_views.push(json!({
            "buffer": 0, "byteOffset": self.length, "byteLength": bytes.len(), "target": ARRAY_BUFFER
        }));
        self.append(&bytes);
        let mut accessor = Map::new();
        accessor.insert("bufferView".into(), json!(buffer_view));
        accessor.insert("componentType".into(), json!(FLOAT));
        accessor.insert("count".into(), json!(values.len() / component_count));
        accessor.insert("type".into(), json!(format!("VEC{component_count}")));
        if include_bounds {
            let mut minimum = vec![f64::INFINITY; component_count];
            let mut maximum = vec![f64::NEG_INFINITY; component_count];
            for (index, value) in floats.iter().enumerate() {
                let component = index % component_count;
                minimum[component] = minimum[component].min(f64::from(*value));
                maximum[component] = maximum[component].max(f64::from(*value));
            }
            accessor.insert("min".into(), json!(minimum));
            accessor.insert("max".into(), json!(maximum));
        }
        self.accessors.push(Json::Object(accessor));
        self.accessors.len() - 1
    }

    /// Append raw bytes, such as an embedded image, and return their buffer view index.
    fn image_view(&mut self, bytes: &[u8]) -> usize {
        let buffer_view = self.buffer_views.len();
        self.buffer_views.push(json!({"buffer": 0, "byteOffset": self.length, "byteLength": bytes.len()}));
        self.append(bytes);
        buffer_view
    }

    fn append(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
        self.length += bytes.len();
    }
}

/// Deduplicates embedded maps into images and textures.
#[derive(Default)]
struct TextureWriter {
    images: Vec<Json>,
    textures: Vec<Json>,
    indexes: std::collections::HashMap<String, usize>,
    uses_webp: bool,
}

impl TextureWriter {
    /// The texture index for a map source, embedding it on first use; `None` for an absent map.
    fn texture(&mut self, source: &Json, writer: &mut BinaryWriter) -> Result<Option<usize>, GlbExportError> {
        let Some(source) = source.as_str().filter(|source| !source.is_empty()) else {
            return Ok(None);
        };
        if let Some(index) = self.indexes.get(source) {
            return Ok(Some(*index));
        }
        let unsupported = || error("GLB textures must be PNG, JPEG, or WebP data URLs.");
        let (header, payload) =
            source.strip_prefix("data:").and_then(|rest| rest.split_once(',')).ok_or_else(unsupported)?;
        let mime_type = header
            .strip_suffix(";base64")
            .filter(|mime| matches!(*mime, "image/png" | "image/jpeg" | "image/webp"))
            .filter(|_| !payload.is_empty())
            .ok_or_else(unsupported)?;
        let bytes = decode_base64(payload).ok_or_else(unsupported)?;
        let buffer_view = writer.image_view(&bytes);
        let index = self.textures.len();
        self.images.push(json!({"bufferView": buffer_view, "mimeType": mime_type}));
        let image = self.images.len() - 1;
        if mime_type == "image/webp" {
            self.uses_webp = true;
            self.textures.push(json!({"extensions": {WEBP_EXTENSION: {"source": image}}}));
        } else {
            self.textures.push(json!({"source": image}));
        }
        self.indexes.insert(source.to_owned(), index);
        Ok(Some(index))
    }
}

/// A glTF metallic-roughness material for a validated FORM material record.
fn export_material(
    material: &Json,
    textures: &mut TextureWriter,
    writer: &mut BinaryWriter,
) -> Result<Json, GlbExportError> {
    let maps = &material["textures"];
    let base_color = textures.texture(&maps["baseColor"], writer)?;
    let normal = textures.texture(&maps["normal"], writer)?;
    let emissive = textures.texture(&maps["emissive"], writer)?;
    let metallic = textures.texture(&maps["metallic"], writer)?;
    let roughness = textures.texture(&maps["roughness"], writer)?;
    let occlusion = textures.texture(&maps["ao"], writer)?;

    let mut pbr = Map::new();
    pbr.insert("baseColorFactor".into(), material["baseColor"].clone());
    pbr.insert("metallicFactor".into(), material["metallic"].clone());
    pbr.insert("roughnessFactor".into(), material["roughness"].clone());
    if let Some(index) = base_color {
        pbr.insert("baseColorTexture".into(), json!({"index": index}));
    }
    if let Some(index) = metallic.or(roughness) {
        pbr.insert("metallicRoughnessTexture".into(), json!({"index": index}));
    }
    let mut result = Map::new();
    result.insert("name".into(), material["name"].clone());
    result.insert("pbrMetallicRoughness".into(), Json::Object(pbr));
    // glTF caps `emissiveFactor` at one; HDR emission is written as a normalized factor and its strength.
    let emissive_channels: Vec<f64> = material["emissive"]
        .as_array()
        .map_or_else(Vec::new, |channels| channels.iter().map(|channel| channel.as_f64().unwrap_or(0.0)).collect());
    let strength = emissive_channels.iter().copied().fold(0.0, f64::max);
    if strength > 1.0 {
        let normalized: Vec<f64> = emissive_channels.iter().map(|channel| channel / strength).collect();
        result.insert("emissiveFactor".into(), json!(normalized));
        result.insert("extensions".into(), json!({EMISSIVE_STRENGTH_EXTENSION: {"emissiveStrength": strength}}));
    } else {
        result.insert("emissiveFactor".into(), material["emissive"].clone());
    }
    result.insert("alphaMode".into(), material["alphaMode"].clone());
    result.insert("doubleSided".into(), json!(true));
    if let Some(index) = normal {
        result.insert("normalTexture".into(), json!({"index": index}));
    }
    if let Some(index) = emissive {
        result.insert("emissiveTexture".into(), json!({"index": index}));
    }
    if let Some(index) = occlusion {
        result.insert("occlusionTexture".into(), json!({"index": index}));
    }
    if material["alphaMode"] == "MASK" {
        result.insert("alphaCutoff".into(), material["alphaCutoff"].clone());
    }
    Ok(Json::Object(result))
}

/// Check counts, finiteness, float32 range, color range, and indices.
fn validate_geometry(data: &GeometryData) -> Result<(), GlbExportError> {
    if data.triangles.is_empty() && data.points.is_empty() {
        return Err(error("Cannot export empty geometry as GLB."));
    }
    let vertex_count = data.positions.len() / 3;
    if !data.positions.len().is_multiple_of(3)
        || data.colors.len() != vertex_count * 3
        || data.alphas.len() != vertex_count
    {
        return Err(error("GLB vertex position, color, and alpha arrays must describe the same vertices."));
    }
    if !data.triangles.len().is_multiple_of(3) {
        return Err(error("GLB triangle topology must contain complete triangles."));
    }
    if !data.normals.is_empty() && data.normals.len() != data.positions.len() {
        return Err(error("GLB normals must describe the same vertices as positions."));
    }
    if !data.uvs.is_empty() && data.uvs.len() != vertex_count * 2 {
        return Err(error("GLB texture coordinates must describe the same vertices as positions."));
    }
    let channels = [&data.positions, &data.colors, &data.alphas, &data.normals, &data.uvs];
    if channels.iter().any(|values| values.iter().any(|value| !value.is_finite())) {
        return Err(error("GLB vertex data must contain only finite numbers."));
    }
    if data.positions.iter().any(|position| !(*position as f32).is_finite()) {
        return Err(error("GLB positions must be representable as float32 values."));
    }
    if data.colors.iter().chain(&data.alphas).any(|color| !(0.0..=1.0).contains(color)) {
        return Err(error(
            "GLB vertex colors and alpha must be from 0 to 1; glTF cannot store HDR vertex colors, so give glowing \
             surfaces an emissive material instead.",
        ));
    }
    if let Some(index) = data.triangles.iter().chain(&data.points).find(|index| **index as usize >= vertex_count) {
        return Err(GlbExportError(format!("GLB topology contains invalid vertex index '{index}'.")));
    }
    Ok(())
}

/// Check one material assignment per triangle and point, each naming a palette entry.
fn validate_assignments(data: &GeometryData, material_count: usize) -> Result<(), GlbExportError> {
    if data.triangle_materials.len() != data.triangles.len() / 3 || data.point_materials.len() != data.points.len() {
        return Err(error("GLB material assignment counts must match triangle and point topology."));
    }
    if let Some(index) =
        data.triangle_materials.iter().chain(&data.point_materials).find(|index| **index as usize >= material_count)
    {
        return Err(GlbExportError(format!("GLB topology contains invalid material index '{index}'.")));
    }
    Ok(())
}

/// Element indexes grouped by material, in order of each material's first use.
fn group_assignments(assignments: &[u32]) -> Vec<(u32, Vec<usize>)> {
    let mut groups: Vec<(u32, Vec<usize>)> = Vec::new();
    for (index, material) in assignments.iter().enumerate() {
        match groups.iter_mut().find(|(existing, _)| existing == material) {
            Some((_, indexes)) => indexes.push(index),
            None => groups.push((*material, vec![index])),
        }
    }
    groups
}

/// The unit face normal of a triangle from its float32 positions; degenerate triangles fail.
fn flat_normal(positions: &[f64], corners: &[u32]) -> Result<[f64; 3], GlbExportError> {
    let point = |corner: u32| -> [f64; 3] {
        let index = corner as usize * 3;
        [0, 1, 2].map(|axis| f64::from(positions[index + axis] as f32))
    };
    let [a, b, c] = [point(corners[0]), point(corners[1]), point(corners[2])];
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let normal =
        [(ab[1] * ac[2]) - (ab[2] * ac[1]), (ab[2] * ac[0]) - (ab[0] * ac[2]), (ab[0] * ac[1]) - (ab[1] * ac[0])];
    let length = DVec3::from_array(normal).length();
    if length == 0.0 {
        return Err(error("GLB export does not support degenerate triangles."));
    }
    Ok(normal.map(|value| value / length))
}

/// Assemble the GLB container: header, space-padded JSON chunk, and zero-padded binary chunk.
fn write_glb(json: &str, binary: &[u8]) -> Vec<u8> {
    let json_length = json.len().next_multiple_of(4);
    let binary_length = binary.len().next_multiple_of(4);
    let total_length = 12 + 8 + json_length + 8 + binary_length;
    let mut output = Vec::with_capacity(total_length);
    for word in [GLB_MAGIC, 2, total_length as u32, json_length as u32, JSON_CHUNK] {
        output.extend_from_slice(&word.to_le_bytes());
    }
    output.extend_from_slice(json.as_bytes());
    output.resize(20 + json_length, b' ');
    output.extend_from_slice(&(binary_length as u32).to_le_bytes());
    output.extend_from_slice(&BINARY_CHUNK.to_le_bytes());
    output.extend_from_slice(binary);
    output.resize(total_length, 0);
    output
}
