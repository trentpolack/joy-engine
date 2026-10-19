// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Editable geometry records, attribute storage, topology edits, and allocation limits.
//! Ported from `src/form/procedural-geometry.ts`.

use crate::{
    math::hypot,
    user_data,
    value::{
        SharedBudget,
        integer_range,
    },
};
use joy_core::math::deterministic;
use serde_json::{
    Value as Json,
    json,
};

pub struct Store {
    pub budget: SharedBudget,
    points: usize,
    faces: usize,
    components: usize,
}

impl Store {
    pub fn new(budget: SharedBudget) -> Self {
        Self { budget, points: 0, faces: 0, components: 0 }
    }

    fn reserve(&mut self, points: usize, faces: usize, components: usize) -> Result<(), String> {
        if points > 150_000usize.saturating_sub(self.points)
            || faces > 180_000usize.saturating_sub(self.faces)
            || components > 2_000_000usize.saturating_sub(self.components)
        {
            return Err("Procedural geometry allocation limit exceeded (150,000 points, 180,000 faces, 2,000,000 attribute components).".into());
        }
        self.points += points;
        self.faces += faces;
        self.components += components;
        Ok(())
    }

    pub fn add_point(&mut self, geometry: &mut Json, position: &[f64]) -> Result<usize, String> {
        let value = json!(position);
        validate_attribute("points", "position", &value)?;
        let attributes = geometry["attributes"]["points"].as_object_mut().unwrap();
        self.reserve(1, 0, attributes.values().map(attribute_size).sum())?;
        let index = attributes["position"]["values"].as_array().unwrap().len();
        for (name, attribute) in attributes {
            let appended = if name == "position" { value.clone() } else { zero(attribute_size(attribute)) };
            attribute["values"].as_array_mut().unwrap().push(appended);
        }
        if let Some(data) = geometry.get_mut("userData").and_then(Json::as_array_mut) {
            data.push(Json::Null);
        }
        Ok(index)
    }

    pub fn add_face(&mut self, geometry: &mut Json, points: &[usize]) -> Result<usize, String> {
        if points.len() != 3 {
            return Err("Faces require three point indices.".into());
        }
        for point in points {
            integer_range(*point as f64, 0.0, count(geometry, "points") as f64 - 1.0)?;
        }
        let components =
            geometry["attributes"]["faces"].as_object().unwrap().values().map(attribute_size).sum::<usize>()
                + geometry["attributes"]["corners"]
                    .as_object()
                    .unwrap()
                    .values()
                    .map(|a| attribute_size(a) * 3)
                    .sum::<usize>();
        self.reserve(0, 1, components)?;
        geometry["faces"].as_array_mut().unwrap().push(json!(points));
        for domain in ["faces", "corners"] {
            for attribute in geometry["attributes"][domain].as_object_mut().unwrap().values_mut() {
                let value = zero(attribute_size(attribute));
                for _ in 0..if domain == "faces" { 1 } else { 3 } {
                    attribute["values"].as_array_mut().unwrap().push(value.clone());
                }
            }
        }
        Ok(count(geometry, "faces") - 1)
    }

    pub fn add_attribute(&mut self, geometry: &mut Json, domain: &str, name: &str, value: &Json) -> Result<(), String> {
        validate_attribute(domain, name, value)?;
        let length = count(geometry, domain);
        let attributes = geometry["attributes"][domain].as_object_mut().unwrap();
        if attributes.contains_key(name) {
            return Err(format!("{domain} attribute '{name}' already exists."));
        }
        if attributes.len() >= 64 {
            return Err("Attribute limit exceeded (64 per domain).".into());
        }
        let size = value_size(value);
        self.reserve(0, 0, length * size)?;
        attributes.insert(name.into(), json!({"size":size, "values": vec![value.clone(); length]}));
        Ok(())
    }

    pub fn set(
        &mut self,
        geometry: &mut Json,
        domain: &str,
        name: &str,
        index: usize,
        value: &Json,
        defer_normals: bool,
    ) -> Result<(), String> {
        let length = count(geometry, domain);
        integer_range(index as f64, 0.0, length as f64 - 1.0)?;
        validate_attribute(domain, name, value)?;
        let size = value_size(value);
        let attributes = geometry["attributes"][domain].as_object_mut().unwrap();
        if !attributes.contains_key(name) {
            if attributes.len() >= 64 {
                return Err("Attribute limit exceeded (64 per domain).".into());
            }
            self.reserve(0, 0, length * size)?;
            attributes.insert(name.into(), json!({"size":size, "values":vec![zero(size);length]}));
        }
        let attribute = attributes.get_mut(name).unwrap();
        if attribute_size(attribute) != size {
            return Err(format!("Attribute '{name}' must retain its {}-component type.", attribute_size(attribute)));
        }
        attribute["values"][index] = value.clone();
        if domain == "points" && name == "position" && !defer_normals {
            for domain in ["points", "faces", "corners"] {
                geometry["attributes"][domain].as_object_mut().unwrap().shift_remove("normal");
            }
        }
        Ok(())
    }

    pub fn set_user_data(&self, geometry: &mut Json, index: usize, value: &Json) -> Result<(), String> {
        let length = count(geometry, "points");
        integer_range(index as f64, 0.0, length as f64 - 1.0)?;
        if geometry.get("userData").is_none() {
            geometry["userData"] = json!(vec![Json::Null; length]);
        }
        geometry["userData"][index] = user_data::copy(value, &self.budget)?;
        Ok(())
    }

    pub fn copy(&mut self, geometry: &Json) -> Result<Json, String> {
        let components = geometry["attributes"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|domain| domain.as_object().unwrap().values())
            .map(|attribute| attribute_size(attribute) * attribute["values"].as_array().unwrap().len())
            .sum();
        self.reserve(count(geometry, "points"), count(geometry, "faces"), components)?;
        let mut result = geometry.clone();
        if let Some(values) = geometry.get("userData").and_then(Json::as_array) {
            result["userData"] =
                Json::Array(values.iter().map(|value| user_data::copy(value, &self.budget)).collect::<Result<_, _>>()?);
        }
        Ok(result)
    }

    pub fn import_mesh(&mut self, mesh: &Json) -> Result<Json, String> {
        let mut geometry = create();
        let positions = mesh["positions"].as_array().unwrap();
        let normals = mesh["normals"].as_array().unwrap();
        let uvs = mesh["uvs"].as_array().unwrap();
        for (index, point) in positions.as_chunks::<3>().0.iter().enumerate() {
            self.add_point(&mut geometry, &point.iter().map(number).collect::<Vec<_>>())?;
            self.set(
                &mut geometry,
                "points",
                "normal",
                index,
                &Json::Array(normals[index * 3..index * 3 + 3].to_vec()),
                false,
            )?;
            self.set(&mut geometry, "points", "uv", index, &json!([uvs[index * 2], uvs[index * 2 + 1], 0.0]), false)?;
        }
        for triangle in mesh["indices"].as_array().unwrap().as_chunks::<3>().0 {
            self.add_face(&mut geometry, &triangle.iter().map(|value| number(value) as usize).collect::<Vec<_>>())?;
        }
        Ok(geometry)
    }

    pub fn grid(&mut self, columns: f64, rows: f64, width: f64, depth: f64) -> Result<Json, String> {
        let columns = integer_range(columns, 1.0, 256.0)?;
        let rows = integer_range(rows, 1.0, 256.0)?;
        if !width.is_finite() || !depth.is_finite() || width <= 0.0 || depth <= 0.0 {
            return Err("Grid dimensions must be positive finite numbers.".into());
        }
        let mut geometry = create();
        for z in 0..=rows {
            for x in 0..=columns {
                let u = x as f64 / columns as f64;
                let v = z as f64 / rows as f64;
                let index = self.add_point(&mut geometry, &[(u - 0.5) * width, 0.0, (v - 0.5) * depth])?;
                self.set(&mut geometry, "points", "uv", index, &json!([u, v, 0.0]), false)?;
            }
        }
        for z in 0..rows {
            for x in 0..columns {
                let a = z * (columns + 1) + x;
                let b = a + columns + 1;
                self.add_face(&mut geometry, &[a, b, a + 1])?;
                self.add_face(&mut geometry, &[a + 1, b, b + 1])?;
            }
        }
        Ok(geometry)
    }
}

pub fn create() -> Json {
    json!({"kind":"geometry","faces":[],"attributes":{"points":{"position":{"size":3,"values":[]}},"faces":{},"corners":{}}})
}
pub fn count(geometry: &Json, domain: &str) -> usize {
    if domain == "points" {
        geometry["attributes"]["points"]["position"]["values"].as_array().unwrap().len()
    } else {
        geometry["faces"].as_array().unwrap().len() * if domain == "corners" { 3 } else { 1 }
    }
}
pub fn attribute_at(geometry: &Json, domain: &str, name: &str, index: f64) -> Result<Json, String> {
    let index = integer_range(index, 0.0, count(geometry, domain) as f64 - 1.0)?;
    let attribute =
        geometry["attributes"][domain].get(name).ok_or_else(|| format!("Unknown {domain} attribute '{name}'."))?;
    Ok(attribute["values"][index].clone())
}
pub fn remove_face(geometry: &mut Json, index: usize) {
    geometry["faces"].as_array_mut().unwrap().remove(index);
    for attribute in geometry["attributes"]["faces"].as_object_mut().unwrap().values_mut() {
        attribute["values"].as_array_mut().unwrap().remove(index);
    }
    for attribute in geometry["attributes"]["corners"].as_object_mut().unwrap().values_mut() {
        attribute["values"].as_array_mut().unwrap().drain(index * 3..index * 3 + 3);
    }
}
pub fn remove_point(geometry: &mut Json, index: usize) {
    for face in (0..count(geometry, "faces")).rev() {
        if geometry["faces"][face].as_array().unwrap().iter().any(|point| number(point) as usize == index) {
            remove_face(geometry, face);
        } else {
            for point in geometry["faces"][face].as_array_mut().unwrap() {
                let old = number(point) as usize;
                if old > index {
                    *point = json!(old - 1);
                }
            }
        }
    }
    if let Some(data) = geometry.get_mut("userData").and_then(Json::as_array_mut) {
        data.remove(index);
    }
    for attribute in geometry["attributes"]["points"].as_object_mut().unwrap().values_mut() {
        attribute["values"].as_array_mut().unwrap().remove(index);
    }
}
pub fn transform(geometry: &mut Json, operation: &str, value: &[f64]) -> Result<(), String> {
    if operation == "scale" && value.contains(&0.0) {
        return Err("scale components must be nonzero.".into());
    }
    for position in geometry["attributes"]["points"]["position"]["values"].as_array_mut().unwrap() {
        let point = numbers(position);
        let result = match operation {
            "translate" => point.iter().zip(value).map(|(a, b)| a + b).collect(),
            "rotate" => rotate(&point, value),
            _ => point.iter().zip(value).map(|(a, b)| a * b).collect(),
        };
        validate_position(&result)?;
        *position = json!(result);
    }
    if operation == "translate" {
        return Ok(());
    }
    let minimum_scale =
        if operation == "scale" { value.iter().map(|v| v.abs()).fold(f64::INFINITY, f64::min) } else { 1.0 };
    for domain in ["points", "faces", "corners"] {
        if let Some(normals) = geometry["attributes"][domain].get_mut("normal") {
            for normal in normals["values"].as_array_mut().unwrap() {
                let source = numbers(normal);
                let transformed = if operation == "rotate" {
                    rotate(&source, value)
                } else {
                    source.iter().zip(value).map(|(component, scale)| component * minimum_scale / scale).collect()
                };
                if transformed.iter().any(|component| !component.is_finite()) {
                    return Err("Transformed normal components must be finite.".into());
                }
                let length = hypot(&transformed);
                if !length.is_finite() {
                    return Err("Transformed normal length must be finite.".into());
                }
                *normal = json!(
                    transformed
                        .iter()
                        .map(|component| if length > 0.0 { component / length } else { 0.0 })
                        .collect::<Vec<_>>()
                );
            }
        }
    }
    if operation == "scale" && value[0] * value[1] * value[2] < 0.0 {
        for face in geometry["faces"].as_array_mut().unwrap() {
            face.as_array_mut().unwrap().swap(1, 2);
        }
        for attribute in geometry["attributes"]["corners"].as_object_mut().unwrap().values_mut() {
            for corners in attribute["values"].as_array_mut().unwrap().as_chunks_mut::<3>().0 {
                corners.swap(1, 2);
            }
        }
    }
    Ok(())
}
pub fn rotate(value: &[f64], angles: &[f64]) -> Vec<f64> {
    let [x, y, z] = [value[0], value[1], value[2]];
    let [rx, ry, rz] = [angles[0], angles[1], angles[2]];
    let y1 = y * deterministic::cos(rx) - z * deterministic::sin(rx);
    let z1 = y * deterministic::sin(rx) + z * deterministic::cos(rx);
    let x2 = x * deterministic::cos(ry) + z1 * deterministic::sin(ry);
    let z2 = -x * deterministic::sin(ry) + z1 * deterministic::cos(ry);
    vec![
        x2 * deterministic::cos(rz) - y1 * deterministic::sin(rz),
        x2 * deterministic::sin(rz) + y1 * deterministic::cos(rz),
        z2,
    ]
}
pub fn number(value: &Json) -> f64 {
    value.as_f64().expect("internally validated numeric geometry")
}
pub fn numbers(value: &Json) -> Vec<f64> {
    value.as_array().unwrap().iter().map(number).collect()
}
fn attribute_size(value: &Json) -> usize {
    value["size"].as_u64().unwrap() as usize
}
fn value_size(value: &Json) -> usize {
    value.as_array().map_or(1, Vec::len)
}
fn zero(size: usize) -> Json {
    if size == 1 { json!(0.0) } else { json!(vec![0.0; size]) }
}
fn validate_position(position: &[f64]) -> Result<(), String> {
    if position.iter().any(|v| !v.is_finite() || v.abs() > 1e6) {
        Err("Coordinates must be finite and within ±1,000,000.".into())
    } else {
        Ok(())
    }
}
fn validate_attribute(domain: &str, name: &str, value: &Json) -> Result<(), String> {
    let bytes = name.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 64
        || !(bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        || bytes.iter().any(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
        || ["__proto__", "constructor", "prototype"].contains(&name)
    {
        return Err("Attribute names require 1–64 identifier characters.".into());
    }
    let values = if let Some(array) = value.as_array() { array.clone() } else { vec![value.clone()] };
    if ![1, 3, 4].contains(&values.len()) || values.iter().any(|value| !value.as_f64().is_some_and(f64::is_finite)) {
        return Err("Attributes require finite scalars, XYZ vectors, or RGBA colors.".into());
    }
    let size = if ["position", "normal", "uv", "rotation", "scale"].contains(&name) {
        3
    } else if name == "color" {
        4
    } else {
        values.len()
    };
    if size != values.len() || (name == "position" && domain != "points") {
        return Err(format!(
            "Attribute '{name}' requires {size} components{}.",
            if name == "position" { " on points" } else { "" }
        ));
    }
    if name == "position" && values.iter().any(|v| number(v).abs() > 1e6) {
        return Err("Coordinates must be within ±1,000,000.".into());
    }
    if name == "color" && !valid_color(&values) {
        return Err("Color attributes require finite, nonnegative RGB and alpha in 0–1.".into());
    }
    Ok(())
}

/// Whether color attribute components are finite, nonnegative RGB (HDR allowed) with alpha, when present, in 0–1.
fn valid_color(values: &[Json]) -> bool {
    values.iter().enumerate().all(|(index, value)| {
        let channel = number(value);
        channel.is_finite() && channel >= 0.0 && (index != 3 || channel <= 1.0)
    })
}
