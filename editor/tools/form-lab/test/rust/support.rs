// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Shared fixtures: the TypeScript tests' sample geometry, GLB readers, and compile helpers.

use joy_engine::form::{GeometryData, default_material};
use joy_engine::serde_json::{Value as Json, json};

/// The `Glass` material of the TypeScript GLB and material tests.
pub fn glass() -> Json {
    json!({"name": "Glass", "baseColor": [0.2, 0.3, 0.4, 0.6], "metallic": 0.7, "roughness": 0.1,
        "emissive": [0.05, 0.1, 0.15], "alphaMode": "MASK", "alphaCutoff": 0.4})
}

/// `sampleGeometry()` from `export-glb.test.js`: one Glass triangle and one Default point, without normals or UVs.
pub fn sample_geometry() -> GeometryData {
    GeometryData {
        positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 2.0, 3.0, 4.0],
        colors: vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.5, 0.6, 0.7],
        alphas: vec![0.25, 0.5, 0.75, 0.8],
        triangles: vec![0, 1, 2],
        points: vec![3],
        materials: vec![default_material(), glass()],
        triangle_materials: vec![1],
        point_materials: vec![0],
        ..GeometryData::default()
    }
}

/// Compile FORM source with overrides and an optional palette, panicking with the diagnostic on failure.
pub fn compile(source: &str, overrides: Json, materials: Option<Json>) -> GeometryData {
    joy_engine::form::compile(source, &overrides, materials.as_ref()).unwrap_or_else(|error| panic!("{error}"))
}

/// A parsed GLB: its JSON chunk and binary chunk.
pub struct ParsedGlb {
    /// The glTF JSON.
    pub json: Json,
    /// The binary chunk.
    pub binary: Vec<u8>,
}

/// Split a GLB into its chunks (`parseGlb`).
pub fn parse_glb(bytes: &[u8]) -> ParsedGlb {
    let word = |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
    let json_length = word(12);
    let json = joy_engine::serde_json::from_str(std::str::from_utf8(&bytes[20..20 + json_length]).unwrap().trim())
        .expect("GLB JSON parses");
    ParsedGlb { json, binary: bytes[28 + json_length..].to_vec() }
}

/// Read a float accessor's values (`readAccessor`).
pub fn read_accessor(parsed: &ParsedGlb, accessor: &Json) -> Vec<f32> {
    let accessor = &parsed.json["accessors"][accessor.as_u64().unwrap() as usize];
    let view = &parsed.json["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
    let components = match accessor["type"].as_str().unwrap() {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        _ => 4,
    };
    let offset =
        view["byteOffset"].as_u64().unwrap_or(0) as usize + accessor["byteOffset"].as_u64().unwrap_or(0) as usize;
    let count = accessor["count"].as_u64().unwrap() as usize * components;
    (0..count)
        .map(|index| f32::from_le_bytes(parsed.binary[offset + index * 4..offset + index * 4 + 4].try_into().unwrap()))
        .collect()
}

/// The text of a fixture written by the TypeScript tool (`test/rust/fixtures/`).
pub fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test/rust/fixtures").join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}
