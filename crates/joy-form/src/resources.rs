// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Reusable mesh and material resource values.

use crate::{
    builder::{
        MeshArrays,
        box_arrays,
        sphere_arrays,
    },
    materials::{
        DEFAULT_MATERIAL_NAME,
        default_material,
        read_materials,
    },
};
use joy_core::math::deterministic;
use serde_json::{
    Value as Json,
    json,
};

const MAX_VERTICES: usize = 150_000;
const MAX_INDICES: usize = 180_000 * 3;

/// Create reusable, centered Y-up geometry in script units.
pub fn mesh_box(width: f64, height: f64, depth: f64) -> Result<Json, String> {
    Ok(mesh_json("Box", &box_arrays(width, height, depth)?))
}

pub fn mesh_sphere(radius: f64, segments: f64) -> Result<Json, String> {
    Ok(mesh_json("Sphere", &sphere_arrays(radius, segments)?))
}

/// Color is linear RGBA; metallic and roughness are normalized factors.
pub fn material_pbr(color: &[f64], metallic: f64, roughness: f64) -> Result<Json, String> {
    let mut material = default_material();
    material["baseColor"] = json!(color);
    material["metallic"] = json!(metallic);
    material["roughness"] = json!(roughness);
    material["alphaMode"] = json!(if color.get(3).is_some_and(|alpha| *alpha < 1.0) { "BLEND" } else { "OPAQUE" });
    // The record is validated under its palette name; report problems against the call's own arguments instead.
    read_material_value(&json!({"kind":"material", "material":material}))
        .map_err(|message| message.replacen(&format!("{DEFAULT_MATERIAL_NAME}."), "materialPbr ", 1))
}

/// Validate and own borrowed mesh arrays. Zero normals request flat shading.
pub fn read_mesh(value: &Json) -> Result<Json, String> {
    let mesh = value.as_object().ok_or("Expected a mesh value.")?;
    if mesh.get("kind").and_then(Json::as_str) != Some("mesh")
        || mesh.get("name").and_then(Json::as_str).is_none_or(|name| name.chars().count() > 120)
    {
        return Err("Mesh requires kind and a name of at most 120 characters.".into());
    }
    let positions = numbers(&value["positions"], MAX_VERTICES * 3, "Mesh positions")?;
    if positions.is_empty() || positions.len() % 3 != 0 || positions.iter().any(|value| value.abs() > 1e6) {
        return Err("Mesh positions require complete XYZ vertices within ±1,000,000.".into());
    }
    let normals = numbers(&value["normals"], MAX_VERTICES * 3, "Mesh normals")?;
    let uvs = numbers(&value["uvs"], MAX_VERTICES * 2, "Mesh UVs")?;
    let indices = numbers(&value["indices"], MAX_INDICES, "Mesh indices")?;
    if normals.len() != positions.len() || uvs.len() != positions.len() / 3 * 2 {
        return Err("Mesh normals and UVs must match the vertex count.".into());
    }
    if indices.is_empty()
        || indices.len() % 3 != 0
        || indices.iter().any(|index| index.fract() != 0.0 || *index < 0.0 || *index >= (positions.len() / 3) as f64)
    {
        return Err("Mesh indices must contain complete triangles with valid vertex indices.".into());
    }
    Ok(json!({"kind":"mesh", "name":value["name"], "positions":value["positions"],
        "normals":value["normals"], "uvs":value["uvs"], "indices":value["indices"]}))
}

pub fn read_material_value(value: &Json) -> Result<Json, String> {
    if !value.is_object() || value["kind"] != "material" || value.get("material").is_none() {
        return Err("Expected a material value.".into());
    }
    let mut materials = read_materials(Some(&json!([value["material"]])))?;
    Ok(json!({"kind":"material", "material":materials.pop().unwrap()}))
}

/// Return an owned mesh: scale, XYZ Euler rotation in radians, then translation.
/// Normals use inverse scale, and reflected transforms reverse triangle winding.
pub fn transform_mesh(mesh: &Json, translation: &[f64], rotation: &[f64], scale: &[f64]) -> Result<Json, String> {
    for vector in [translation, rotation, scale] {
        if vector.len() != 3 || vector.iter().any(|component| !component.is_finite()) {
            return Err("Transforms require finite XYZ vectors.".into());
        }
    }
    if scale.iter().any(|component| component.abs() < 1e-9) {
        return Err("Mesh scale must be nonzero.".into());
    }
    let mut result = read_mesh(mesh)?;
    let mut positions = numbers(&result["positions"], MAX_VERTICES * 3, "Mesh positions")?;
    let mut normals = numbers(&result["normals"], MAX_VERTICES * 3, "Mesh normals")?;
    let (ax, cx) = deterministic::sin_cos(rotation[0]);
    let (ay, cy) = deterministic::sin_cos(rotation[1]);
    let (az, cz) = deterministic::sin_cos(rotation[2]);
    let rotate = |x: f64, y: f64, z: f64| {
        let y1 = y * cx - z * ax;
        let z1 = y * ax + z * cx;
        let x2 = x * cy + z1 * ay;
        let z2 = -x * ay + z1 * cy;
        [x2 * cz - y1 * az, x2 * az + y1 * cz, z2]
    };
    for (position, normal) in positions.as_chunks_mut::<3>().0.iter_mut().zip(normals.as_chunks_mut::<3>().0.iter_mut())
    {
        let rotated_position = rotate(position[0] * scale[0], position[1] * scale[1], position[2] * scale[2]);
        let rotated_normal = rotate(normal[0] / scale[0], normal[1] / scale[1], normal[2] / scale[2]);
        let length =
            deterministic::hypot(deterministic::hypot(rotated_normal[0], rotated_normal[1]), rotated_normal[2]);
        for axis in 0..3 {
            position[axis] = rotated_position[axis] + translation[axis];
            normal[axis] = if length > 0.0 { rotated_normal[axis] / length } else { 0.0 };
        }
    }
    result["positions"] = json!(positions);
    result["normals"] = json!(normals);
    if scale[0] * scale[1] * scale[2] < 0.0 {
        for triangle in result["indices"].as_array_mut().unwrap().as_chunks_mut::<3>().0 {
            triangle.swap(1, 2);
        }
    }
    read_mesh(&result)
}

fn mesh_json(name: &str, mesh: &MeshArrays) -> Json {
    json!({"kind":"mesh", "name":name, "positions":mesh.positions,
        "normals":mesh.normals, "uvs":mesh.uvs, "indices":mesh.indices})
}

/// A validated mesh resource's arrays.
pub fn mesh_arrays(mesh: &Json) -> MeshArrays {
    let numbers = |name: &str| -> Vec<f64> {
        mesh[name].as_array().map(|values| values.iter().filter_map(Json::as_f64).collect()).unwrap_or_default()
    };
    MeshArrays {
        positions: numbers("positions"),
        normals: numbers("normals"),
        uvs: numbers("uvs"),
        indices: numbers("indices").into_iter().map(|index| index as u32).collect(),
    }
}

fn numbers(value: &Json, limit: usize, name: &str) -> Result<Vec<f64>, String> {
    let error = || format!("{name} must be finite numbers within the geometry limit.");
    let values = value.as_array().filter(|values| values.len() <= limit).ok_or_else(error)?;
    values.iter().map(|value| value.as_f64().filter(|value| value.is_finite()).ok_or_else(error)).collect()
}
