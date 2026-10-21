// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Parameter declarations, override validation, and clamping.

use crate::ast::ParameterPresentation;
use crate::parser::{
    PARAMETER_MAXIMUM,
    PARAMETER_MINIMUM,
    PARAMETER_STEP,
};
use crate::{
    resources,
    value::Value,
};
use serde_json::{
    Value as Json,
    json,
};

// Descriptor fields editors read beside `name`, `type`, bounds, and values.
const DESCRIPTOR_LABEL: &str = "label";
const DESCRIPTOR_GROUP: &str = "group";
const DESCRIPTOR_INFO: &str = "info";
const DESCRIPTOR_KIND: &str = "kind";
const DESCRIPTOR_OPTIONS: &str = "options";

// Whole-number parameter kinds.
const KIND_INT: &str = "int";
const KIND_BOOL: &str = "bool";
const KIND_SEED: &str = "seed";
const KIND_ENUM: &str = "enum";

/// Largest default seed bound: the nonnegative 31-bit range every host stores exactly.
const SEED_MAXIMUM: f64 = 2_147_483_647.0;

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
    presentation: &ParameterPresentation,
    supplied: &Json,
) -> Result<(Json, Value), String> {
    let default = &values[0];
    if let Value::Object(object) = default {
        if values.len() != 1 || !metadata.is_empty() {
            return Err("Mesh and material parameters do not accept numeric bounds.".into());
        }
        if presentation.kind.is_some() || !presentation.options.is_empty() {
            return Err("Mesh and material parameters do not accept a kind or options.".into());
        }
        let resource = object.borrow();
        let kind = resource["kind"].as_str().unwrap_or("");
        let (default, value) = if kind == "mesh" {
            (resources::read_mesh(&resource)?, resources::read_mesh(supplied)?)
        } else {
            (resources::read_material_value(&resource)?, resources::read_material_value(supplied)?)
        };
        let mut descriptor = json!({"name":name,"type":kind,"defaultValue":default,"value":value});
        describe_presentation(&mut descriptor, presentation, None);
        return Ok((descriptor, Value::object(value)));
    }
    let kind = match default {
        Value::Array(v) if v.len() == 3 => "vector",
        Value::Array(_) => "color",
        _ => "scalar",
    };
    let role = ParameterRole::read(presentation)?;
    if role.is_some() && kind != "scalar" {
        return Err("Only scalar parameters accept an int, bool, seed, or enum kind.".into());
    }

    // Booleans and options derive their bounds; seeds default to the nonnegative 31-bit range; whole numbers step by one.
    let derived = role.and_then(|role| role.derived_bounds(presentation.options.len()));
    if derived.is_some() && (values.len() == 4 || !metadata.is_empty()) {
        return Err("Boolean and option parameters derive their bounds; remove min, max, and step.".into());
    }
    let fallback = |key: &str| -> f64 {
        match (role, key) {
            (Some(ParameterRole::Seed), PARAMETER_MINIMUM) => 0.0,
            (Some(ParameterRole::Seed), PARAMETER_MAXIMUM) => SEED_MAXIMUM,
            (Some(_), PARAMETER_STEP) => 1.0,
            (None, PARAMETER_MINIMUM) if kind == "color" => 0.0,
            (None, PARAMETER_MAXIMUM) if kind == "color" => 1.0,
            (None, PARAMETER_STEP) if kind == "color" => 0.01,
            _ => f64::NAN,
        }
    };
    let bound = |key: &str, index: usize| -> Result<f64, String> {
        if let Some((minimum, maximum)) = derived {
            return Ok(match key {
                PARAMETER_MINIMUM => minimum,
                PARAMETER_MAXIMUM => maximum,
                _ => 1.0,
            });
        }
        if values.len() == 4 {
            values[index].scalar()
        } else {
            metadata
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.scalar())
                .unwrap_or_else(|| Value::Number(fallback(key)).scalar())
        }
    };
    let min = bound(PARAMETER_MINIMUM, 1)?;
    let max = bound(PARAMETER_MAXIMUM, 2)?;
    let step = bound(PARAMETER_STEP, 3)?;
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
    if role.is_some() && [min, max, step, defaults[0]].iter().any(|v| v.fract() != 0.0) {
        return Err("Int, bool, seed, and enum parameters require whole-number defaults, bounds, and steps.".into());
    }
    // Host overrides follow the numeric-component boundary, not declaration decoding. Booleans also accept JSON flags.
    let supplied = if let Some(n) = supplied.as_f64() {
        Value::Number(n)
    } else if let Some(a) = supplied.as_array() {
        Value::Array(a.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect())
    } else if role == Some(ParameterRole::Bool)
        && let Some(flag) = supplied.as_bool()
    {
        Value::Number(if flag { 1.0 } else { 0.0 })
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
            // Whole-number kinds round before clamping, so a dragged control always lands on a valid choice.
            let v = if role.is_some() { v.round() } else { v };
            v.max(min).min(upper)
        })
        .collect();
    let value = if matches!(default, Value::Array(_)) { Value::Array(clamped) } else { Value::Number(clamped[0]) };
    let mut descriptor = json!({"name":name,"type":kind,"min":min,"max":max,"step":step});
    descriptor["defaultValue"] = default.json();
    descriptor["value"] = value.json();
    describe_presentation(&mut descriptor, presentation, role);
    Ok((descriptor, value))
}

/// Add the optional presentation fields (`label`, `group`, `info`, `kind`, and `options`) to a descriptor.
fn describe_presentation(descriptor: &mut Json, presentation: &ParameterPresentation, role: Option<ParameterRole>) {
    let Some(fields) = descriptor.as_object_mut() else {
        return;
    };
    let text_fields = [
        (DESCRIPTOR_LABEL, &presentation.label),
        (DESCRIPTOR_GROUP, &presentation.group),
        (DESCRIPTOR_INFO, &presentation.info),
    ];
    for (key, text) in text_fields {
        if let Some(text) = text {
            fields.insert(key.into(), Json::String(text.clone()));
        }
    }
    if let Some(role) = role {
        fields.insert(DESCRIPTOR_KIND.into(), Json::String(role.name().into()));
    }
    if role == Some(ParameterRole::Enum) {
        fields.insert(DESCRIPTOR_OPTIONS.into(), json!(presentation.options));
    }
}

/// A scalar parameter's whole-number control, from `meta(kind = "...")` or implied by `options`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParameterRole {
    /// A whole number between the declared bounds.
    Int,
    /// Zero or one, shown as a toggle.
    Bool,
    /// A random seed, shown with a reroll control.
    Seed,
    /// An index into the declared option labels, shown as a choice.
    Enum,
}

impl ParameterRole {
    /// The role declared by `kind` and `options`, validating that they agree.
    fn read(presentation: &ParameterPresentation) -> Result<Option<Self>, String> {
        let declared = match presentation.kind.as_deref() {
            None => None,
            Some(KIND_INT) => Some(Self::Int),
            Some(KIND_BOOL) => Some(Self::Bool),
            Some(KIND_SEED) => Some(Self::Seed),
            Some(KIND_ENUM) => Some(Self::Enum),
            Some(other) => {
                return Err(format!("Parameter kind '{other}' must be int, bool, seed, or enum."));
            }
        };
        match (declared, presentation.options.is_empty()) {
            (None | Some(Self::Enum), false) => Ok(Some(Self::Enum)),
            (Some(Self::Enum), true) => Err("Enum parameters require options.".into()),
            (Some(_), false) => Err("Only enum parameters accept options.".into()),
            (role, true) => Ok(role),
        }
    }

    /// The serialized kind name.
    fn name(self) -> &'static str {
        match self {
            Self::Int => KIND_INT,
            Self::Bool => KIND_BOOL,
            Self::Seed => KIND_SEED,
            Self::Enum => KIND_ENUM,
        }
    }

    /// Fixed `(min, max)` bounds for the roles that derive them: booleans and options.
    fn derived_bounds(self, option_count: usize) -> Option<(f64, f64)> {
        match self {
            Self::Bool => Some((0.0, 1.0)),
            Self::Enum => Some((0.0, option_count.saturating_sub(1) as f64)),
            Self::Int | Self::Seed => None,
        }
    }
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
