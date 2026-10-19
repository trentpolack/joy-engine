// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Parameter declarations, override validation, and clamping. Ported from `src/form/parameters.ts`.

use crate::{
    resources,
    value::Value,
};
use serde_json::{
    Value as Json,
    json,
};

pub fn read(value: &Value) -> Result<Value, String> {
    if matches!(value, Value::Number(_) | Value::Array(_) | Value::Object(_)) {
        return read_json(&value.json());
    }
    Err("Parameters require finite scalars, XYZ vectors, RGBA colors, meshes, or materials.".into())
}
pub fn read_json(value: &Json) -> Result<Value, String> {
    if let Some(n) = value.as_f64()
        && n.is_finite()
    {
        return Ok(Value::Number(n));
    }
    if let Some(v) = value.as_array()
        && [3, 4].contains(&v.len())
    {
        let numbers: Option<Vec<f64>> = v.iter().map(Json::as_f64).collect();
        if let Some(v) = numbers
            && v.iter().all(|n| n.is_finite())
        {
            return Ok(Value::Array(v));
        }
    }
    match value.get("kind").and_then(Json::as_str) {
        Some("mesh") => return Ok(Value::object(resources::read_mesh(value)?)),
        Some("material") => return Ok(Value::object(resources::read_material_value(value)?)),
        _ => {}
    }
    Err("Parameters require finite scalars, XYZ vectors, RGBA colors, meshes, or materials.".into())
}

pub fn create(
    name: &str,
    values: &[Value],
    metadata: &[(String, Value)],
    supplied: &Json,
) -> Result<(Json, Value), String> {
    let default = &values[0];
    if let Value::Object(object) = default {
        if values.len() != 1 || !metadata.is_empty() {
            return Err("Mesh and material parameters do not accept numeric bounds.".into());
        }
        let resource = object.borrow();
        let kind = resource["kind"].as_str().unwrap_or("");
        let (default, value) = if kind == "mesh" {
            (resources::read_mesh(&resource)?, resources::read_mesh(supplied)?)
        } else {
            (resources::read_material_value(&resource)?, resources::read_material_value(supplied)?)
        };
        return Ok((json!({"name":name,"type":kind,"defaultValue":default,"value":value}), Value::object(value)));
    }
    let kind = match default {
        Value::Array(v) if v.len() == 3 => "vector",
        Value::Array(_) => "color",
        _ => "scalar",
    };
    let bound = |key: &str, index: usize, fallback: f64| -> Result<f64, String> {
        if values.len() == 4 {
            values[index].scalar()
        } else {
            metadata
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.scalar())
                .unwrap_or_else(|| Value::Number(fallback).scalar())
        }
    };
    let min = bound("min", 1, if kind == "color" { 0.0 } else { f64::NAN })?;
    let max = bound("max", 2, if kind == "color" { 1.0 } else { f64::NAN })?;
    let step = bound("step", 3, if kind == "color" { 0.01 } else { f64::NAN })?;
    let defaults = components(default)?;
    // Color RGB may exceed one for HDR glow when the author raises `max`; alpha stays opacity in 0–1.
    if kind == "color"
        && (min < 0.0 || defaults.iter().any(|v| *v < 0.0) || defaults.get(3).is_some_and(|alpha| *alpha > 1.0))
    {
        return Err("Color parameters require nonnegative components and bounds, with alpha in 0–1.".into());
    }
    if min > max || step <= 0.0 || defaults.iter().any(|v| *v < min || *v > max) {
        return Err("Parameter requires min ≤ default ≤ max and a positive step.".into());
    }
    // Host overrides follow the numeric-component boundary, not declaration decoding.
    let supplied = if let Some(n) = supplied.as_f64() {
        Value::Number(n)
    } else if let Some(a) = supplied.as_array() {
        Value::Array(a.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect())
    } else if supplied.is_string() || supplied.is_boolean() {
        return Err("Expected a finite scalar; parameter metadata requires min, max, and step.".into());
    } else {
        return Err("Expected numeric parameter components.".into());
    };
    let components = components(&supplied)?;
    if matches!(default, Value::Array(_)) != matches!(supplied, Value::Array(_)) || components.len() != defaults.len() {
        return Err("Parameter override must match the declared components.".into());
    }
    let clamped: Vec<f64> = components
        .into_iter()
        .enumerate()
        .map(|(index, v)| {
            let upper = if kind == "color" && index == 3 { max.min(1.0) } else { max };
            v.max(min).min(upper)
        })
        .collect();
    let value = if matches!(default, Value::Array(_)) { Value::Array(clamped) } else { Value::Number(clamped[0]) };
    Ok((
        json!({"name":name,"type":kind,"min":min,"max":max,"step":step,"defaultValue":default.json(),"value":value.json()}),
        value,
    ))
}
fn components(value: &Value) -> Result<Vec<f64>, String> {
    match value {
        Value::Array(v) => {
            if v.len() != 3 && v.len() != 4 {
                return Err("Parameters require three vector or four color components.".into());
            }
            v.iter().map(|n| Value::Number(*n).scalar()).collect()
        }
        Value::Number(_) => Ok(vec![value.scalar()?]),
        _ => Err("Expected numeric parameter components.".into()),
    }
}
