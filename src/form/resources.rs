// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Reusable mesh and material resource values. Ported from `src/form/resources.ts`.

use crate::form::{
    builder::Builder,
    materials::{
        default_material,
        read_materials,
    },
    value::Budget,
};
use serde_json::{
    Value as Json,
    json,
};
use std::{
    cell::RefCell,
    rc::Rc,
};

const MAX_VERTICES: usize = 150_000;
const MAX_INDICES: usize = 180_000 * 3;

/// Create reusable, centered Y-up geometry in script units.
pub fn mesh_box(width: f64, height: f64, depth: f64) -> Result<Json, String> {
    let mut builder = Builder::new(None, Rc::new(RefCell::new(Budget::default())))?;
    builder.box_geometry(&[0.0, 0.0, 0.0, width, height, depth])?;
    Ok(from_builder("Box", builder))
}

pub fn mesh_sphere(radius: f64, segments: f64) -> Result<Json, String> {
    let mut builder = Builder::new(None, Rc::new(RefCell::new(Budget::default())))?;
    builder.sphere(&[0.0, 0.0, 0.0, radius, segments])?;
    Ok(from_builder("Sphere", builder))
}

/// Color is linear RGBA; metallic and roughness are normalized factors.
pub fn material_pbr(color: &[f64], metallic: f64, roughness: f64) -> Result<Json, String> {
    let mut material = default_material();
    material["baseColor"] = json!(color);
    material["metallic"] = json!(metallic);
    material["roughness"] = json!(roughness);
    material["alphaMode"] = json!(if color.get(3).is_some_and(|alpha| *alpha < 1.0) { "BLEND" } else { "OPAQUE" });
    read_material_value(&json!({"kind":"material", "material":material}))
}

/// Validate and own borrowed mesh arrays. Zero normals request flat shading.
pub fn read_mesh(value: &Json) -> Result<Json, String> {
    let mesh = value.as_object().ok_or("Expected a mesh value.")?;
    if mesh.get("kind").and_then(Json::as_str) != Some("mesh")
        || mesh.get("name").and_then(Json::as_str).is_none_or(|name| name.encode_utf16().count() > 120)
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
    let (ax, cx) = rotation[0].sin_cos();
    let (ay, cy) = rotation[1].sin_cos();
    let (az, cz) = rotation[2].sin_cos();
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
        let length = rotated_normal[0].hypot(rotated_normal[1]).hypot(rotated_normal[2]);
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

fn from_builder(name: &str, builder: Builder) -> Json {
    let data = builder.data;
    json!({"kind":"mesh", "name":name, "positions":data.positions,
        "normals":data.normals, "uvs":data.uvs, "indices":data.triangles})
}

fn numbers(value: &Json, limit: usize, name: &str) -> Result<Vec<f64>, String> {
    let error = || format!("{name} must be finite numbers within the geometry limit.");
    let values = value.as_array().filter(|values| values.len() <= limit).ok_or_else(error)?;
    values.iter().map(|value| value.as_f64().filter(|value| value.is_finite()).ok_or_else(error)).collect()
}
