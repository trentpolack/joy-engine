// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Realizes editable geometry and instance collections into the output builder.

use super::store::{
    count,
    number,
    numbers,
    rotate,
};
use crate::{
    builder::Builder,
    math::hypot,
    value::DataRef,
};
use serde_json::Value as Json;
use std::collections::HashSet;

/// Realization preserves logical data separately and only splits rendering seams here.
pub fn emit(value: &Json, builder: &mut Builder, material_index: Option<u32>) -> Result<(), String> {
    if value["kind"] == "geometry" {
        return emit_geometry(value, builder, None);
    }
    let sites = &value["sites"]["attributes"]["points"];
    let saved_material = builder.material_index;
    let saved_color = builder.color.clone();
    let saved_data = builder.user_data.clone();
    if let Some(index) = material_index {
        builder.material_index = index;
    }
    let result = (|| {
        for index in 0..sites["position"]["values"].as_array().unwrap().len() {
            let position = numbers(&sites["position"]["values"][index]);
            let rotation = site_attribute(sites, "rotation", index, &[0.0, 0.0, 0.0]);
            let scale = site_attribute(sites, "scale", index, &[1.0, 1.0, 1.0]);
            let tint = site_attribute(sites, "color", index, &[1.0, 1.0, 1.0, 1.0]);
            builder.color = saved_color.iter().zip(tint).map(|(a, b)| a * b).collect();
            builder.user_data = if value["sites"].get("userData").is_some() {
                Some(DataRef::new(value["sites"]["userData"][index].clone()))
            } else {
                saved_data.clone()
            };
            if scale.iter().any(|v| v.abs() < 1e-9) {
                return Err("Instance scale must be nonzero.".into());
            }
            emit_geometry(&value["source"], builder, Some(Transform { position, rotation, scale }))?;
        }
        Ok(())
    })();
    builder.material_index = saved_material;
    builder.color = saved_color;
    builder.user_data = saved_data;
    result
}

struct Transform {
    position: Vec<f64>,
    rotation: Vec<f64>,
    scale: Vec<f64>,
}

struct Emission<'a> {
    geometry: &'a Json,
    transform: Option<Transform>,
    color: Vec<f64>,
    user_data: Option<DataRef>,
}
impl Emission<'_> {
    fn vertex(
        &self,
        builder: &mut Builder,
        point: usize,
        corner: Option<usize>,
        face: Option<usize>,
    ) -> Result<u32, String> {
        let attributes = &self.geometry["attributes"];
        let attribute = |name: &str, fallback: &[f64]| -> Vec<f64> {
            corner
                .and_then(|index| attributes["corners"].get(name).map(|a| &a["values"][index]))
                .or_else(|| face.and_then(|index| attributes["faces"].get(name).map(|a| &a["values"][index])))
                .or_else(|| attributes["points"].get(name).map(|a| &a["values"][point]))
                .map(numbers)
                .unwrap_or_else(|| fallback.to_vec())
        };
        let mut position = numbers(&attributes["points"]["position"]["values"][point]);
        let mut normal = attribute("normal", &[0.0, 0.0, 0.0]);
        let scale = normal.iter().map(|v| v.abs()).fold(0.0, f64::max);
        if scale != 0.0 {
            for value in &mut normal {
                *value /= scale;
            }
        }
        let length = hypot(&normal);
        if length != 0.0 {
            for value in &mut normal {
                *value /= length;
            }
        }
        if let Some(transform) = &self.transform {
            let scaled: Vec<f64> = position.iter().zip(&transform.scale).map(|(a, b)| a * b).collect();
            position =
                rotate(&scaled, &transform.rotation).iter().zip(&transform.position).map(|(a, b)| a + b).collect();
            let scaled: Vec<f64> = normal.iter().zip(&transform.scale).map(|(a, b)| a / b).collect();
            normal = rotate(&scaled, &transform.rotation);
            let length = hypot(&normal);
            for value in &mut normal {
                *value = if length != 0.0 { *value / length } else { 0.0 };
            }
        }
        let tint = attribute("color", &[1.0, 1.0, 1.0, 1.0]);
        builder.color = self.color.iter().zip(tint).map(|(a, b)| a * b).collect();
        builder.user_data = if self.geometry.get("userData").is_some() {
            Some(DataRef::new(self.geometry["userData"][point].clone()))
        } else {
            self.user_data.clone()
        };
        builder.normal = normal;
        builder.uv = attribute("uv", &[0.0, 0.0, 0.0])[..2].to_vec();
        builder.vertex(position[0], position[1], position[2])
    }
}

fn emit_geometry(geometry: &Json, builder: &mut Builder, transform: Option<Transform>) -> Result<(), String> {
    let saved_color = builder.color.clone();
    let saved_normal = builder.normal.clone();
    let saved_uv = builder.uv.clone();
    let saved_data = builder.user_data.clone();
    let emission = Emission { geometry, transform, color: saved_color.clone(), user_data: saved_data.clone() };
    let result = (|| {
        let faces = geometry["faces"].as_array().unwrap();
        let used: HashSet<usize> =
            faces.iter().flat_map(|face| face.as_array().unwrap().iter().map(|point| number(point) as usize)).collect();
        let split = ["corners", "faces"].iter().any(|domain| {
            geometry["attributes"][domain]
                .as_object()
                .unwrap()
                .keys()
                .any(|name| ["normal", "uv", "color"].contains(&name.as_str()))
        });
        let first = (builder.data.positions.len() / 3) as u32;
        let point_count = count(geometry, "points");
        if !split {
            for point in 0..point_count {
                emission.vertex(builder, point, None, None)?;
            }
        }
        for (face_index, face) in faces.iter().enumerate() {
            let mut indices = [0; 3];
            for (corner, point) in face.as_array().unwrap().iter().enumerate() {
                indices[corner] = if split {
                    emission.vertex(builder, number(point) as usize, Some(face_index * 3 + corner), Some(face_index))?
                } else {
                    first + number(point) as u32
                };
            }
            if emission.transform.as_ref().is_some_and(|t| t.scale[0] * t.scale[1] * t.scale[2] < 0.0) {
                indices.swap(1, 2);
            }
            builder.face(indices[0], indices[1], indices[2])?;
        }
        for point in 0..point_count {
            if !used.contains(&point) {
                let index = if split { emission.vertex(builder, point, None, None)? } else { first + point as u32 };
                builder.data.points.push(index);
                builder.data.point_materials.push(builder.material_index);
            }
        }
        Ok(())
    })();
    builder.color = saved_color;
    builder.normal = saved_normal;
    builder.uv = saved_uv;
    builder.user_data = saved_data;
    result
}
fn site_attribute(sites: &Json, name: &str, index: usize, fallback: &[f64]) -> Vec<f64> {
    sites.get(name).map(|attribute| numbers(&attribute["values"][index])).unwrap_or_else(|| fallback.to_vec())
}
