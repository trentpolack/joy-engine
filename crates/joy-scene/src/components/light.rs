// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The `light` component: a world-space point light at the entity position.

use joy_core::{
    JsonRecord,
    LinearColor,
};
use joy_gameplay::{
    ComponentFieldKind,
    ComponentFieldSchema,
    ComponentSchema,
    component_category,
    component_preset,
    component_type,
    number_to_json,
    read_vector,
};
use serde_json::{
    Value,
    json,
};

use super::{
    ComponentError,
    preset,
    text,
};

/// Values assumed for omitted `light` fields. Range is meters; intensity scales linear color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightComponentDefaults {
    /// Whether the light contributes.
    pub enabled: bool,
    /// Linear color before intensity.
    pub color: LinearColor,
    /// Linear intensity multiplier.
    pub intensity: f32,
    /// Falloff range in meters.
    pub range: f32,
    /// Whether the light casts shadows.
    pub casts_shadow: bool,
}

/// Engine defaults for `light` components.
pub const LIGHT_COMPONENT_DEFAULTS: LightComponentDefaults = LightComponentDefaults {
    enabled: true,
    color: LinearColor::WHITE,
    intensity: 1.0,
    range: 10.0,
    casts_shadow: true,
};

/// Interpreted `light` component: a point light at the entity position (rotation and scale are ignored).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightComponent {
    /// Whether the light contributes; false only when `enabled` is exactly `false`.
    pub enabled: bool,
    /// Linear color before intensity.
    pub color: LinearColor,
    /// Linear intensity multiplier; nonnegative.
    pub intensity: f32,
    /// Falloff range in meters; positive.
    pub range: f32,
    /// Whether the light casts shadows; false only when `castsShadow` is exactly `false`.
    pub casts_shadow: bool,
}

impl LightComponent {
    /// Interpret an authored `light` record.
    ///
    /// For enabled lights, a present `intensity` or `range` must be a finite number, intensity and color must be
    /// nonnegative, and range must be positive. A malformed color falls back to the default. Disabled lights are not
    /// validated (matching `LevelScene`): non-numeric values fall back to defaults; numbers are returned as authored.
    pub fn read(record: &JsonRecord) -> Result<Self, ComponentError> {
        let enabled = record.get("enabled") != Some(&Value::Bool(false));
        let casts_shadow = record.get("castsShadow") != Some(&Value::Bool(false));
        let color = record
            .get("color")
            .and_then(read_vector)
            .map_or(LIGHT_COMPONENT_DEFAULTS.color, |color| LinearColor::from_rgb_array(color.to_array()));
        let intensity = light_scalar(record.get("intensity"), LIGHT_COMPONENT_DEFAULTS.intensity);
        let range = light_scalar(record.get("range"), LIGHT_COMPONENT_DEFAULTS.range);

        if !enabled {
            return Ok(Self {
                enabled,
                color,
                intensity: intensity.unwrap_or(LIGHT_COMPONENT_DEFAULTS.intensity),
                range: range.unwrap_or(LIGHT_COMPONENT_DEFAULTS.range),
                casts_shadow,
            });
        }

        let intensity = intensity.ok_or(ComponentError::NonFiniteLightValue)?;
        let range = range.ok_or(ComponentError::NonFiniteLightValue)?;
        if intensity < 0.0 || range <= 0.0 || color.rgb_array().iter().any(|channel| *channel < 0.0) {
            return Err(ComponentError::InvalidLight);
        }
        Ok(Self { enabled, color, intensity, range, casts_shadow })
    }

    /// Linear radiance: color multiplied by intensity.
    pub fn radiance(&self) -> LinearColor {
        self.color.scaled(self.intensity)
    }
}

/// World-space point light derived by `LevelScene` from the entity position.
pub(super) fn light_schema() -> ComponentSchema {
    ComponentSchema {
        kind: component_type::LIGHT.to_owned(),
        label: "Point Light".to_owned(),
        description: "Illuminates the scene from the entity position. Rotation and scale are ignored.".to_owned(),
        category: component_category::LIGHTING.to_owned(),
        fields: vec![
            ComponentFieldSchema {
                default: Some(Value::Bool(LIGHT_COMPONENT_DEFAULTS.enabled)),
                ..ComponentFieldSchema::new("enabled", ComponentFieldKind::Boolean)
            },
            ComponentFieldSchema {
                min: Some(0.0),
                default: Some(Value::Array(
                    LIGHT_COMPONENT_DEFAULTS.color.rgb_array().into_iter().map(number_to_json).collect(),
                )),
                description: text("Linear RGB before intensity."),
                ..ComponentFieldSchema::new("color", ComponentFieldKind::Color)
            },
            ComponentFieldSchema {
                unit: text("linear"),
                min: Some(0.0),
                step: Some(0.25),
                default: Some(number_to_json(LIGHT_COMPONENT_DEFAULTS.intensity)),
                ..ComponentFieldSchema::new("intensity", ComponentFieldKind::Number)
            },
            ComponentFieldSchema {
                unit: text("m"),
                min: Some(0.0),
                min_exclusive: true,
                step: Some(0.5),
                default: Some(number_to_json(LIGHT_COMPONENT_DEFAULTS.range)),
                ..ComponentFieldSchema::new("range", ComponentFieldKind::Number)
            },
            ComponentFieldSchema {
                label: text("casts shadow"),
                default: Some(Value::Bool(LIGHT_COMPONENT_DEFAULTS.casts_shadow)),
                ..ComponentFieldSchema::new("castsShadow", ComponentFieldKind::Boolean)
            },
        ],
        presets: vec![preset(
            component_preset::LIGHT_POINT,
            "Point Light",
            json!({"enabled": true, "color": [1, 0.88, 0.7], "intensity": 4, "range": 12, "castsShadow": true}),
        )],
        requires: Vec::new(),
    }
}

/// Read an optional finite light scalar; `None` means present but not a finite number.
fn light_scalar(value: Option<&Value>, fallback: f32) -> Option<f32> {
    match value {
        None => Some(fallback),
        Some(value) => value.as_f64().map(|number| number as f32).filter(|number| number.is_finite()),
    }
}
