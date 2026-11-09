// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The authored `environment` record: a level's sky (atmosphere, clouds, light shafts, and sky occlusion), its solar
//! clock (time of day, day length, latitude, season, and north), and its wind, so levels set their look and weather
//! instead of games hard-coding them.
//!
//! The record holds two optional objects in the engine's existing document spellings: `simulation`
//! (`joy_environment::EnvironmentOptions`, camelCase, including `wind`) and `sky` (`joy_renderer::SkyOptions`). Omitted
//! fields keep their defaults; unknown fields and out-of-range values are rejected. A level uses the first entity, in
//! authored order, that carries the component.

use joy_core::JsonRecord;
use joy_environment::EnvironmentOptions;
use joy_gameplay::{
    ComponentFieldKind,
    ComponentFieldSchema,
    ComponentSchema,
    LevelData,
    component_category,
    component_preset,
    component_type,
};
use joy_renderer::SkyOptions;
use serde_json::{
    Value,
    json,
};

use super::{
    ComponentError,
    preset,
    text,
};

/// Name of the record's solar clock and wind object.
const SIMULATION_FIELD: &str = "simulation";
/// Name of the record's sky object.
const SKY_FIELD: &str = "sky";

/// A validated `environment` record.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EnvironmentComponent {
    /// The solar clock and wind the environment simulation runs.
    pub simulation: EnvironmentOptions,
    /// The sky's scattering, clouds, light shafts, and sky occlusion.
    pub sky: SkyOptions,
}

impl EnvironmentComponent {
    /// Read and validate an authored `environment` record.
    pub fn read(record: &JsonRecord) -> Result<Self, ComponentError> {
        let simulation = match record.get(SIMULATION_FIELD) {
            None | Some(Value::Null) => EnvironmentOptions::default(),
            Some(value) => EnvironmentOptions::from_json(value)?,
        };
        let sky = match record.get(SKY_FIELD) {
            None | Some(Value::Null) => SkyOptions::default(),
            Some(value) => SkyOptions::from_json(value)?,
        };
        Ok(Self {
            simulation,
            sky,
        })
    }

    /// The level's environment: the first entity's `environment` record, or `None` when no entity carries one.
    pub fn from_level(level: &LevelData) -> Result<Option<Self>, ComponentError> {
        level
            .entities
            .iter()
            .find_map(|entity| entity.components.get(component_type::ENVIRONMENT))
            .map(Self::read)
            .transpose()
    }
}

/// The `environment` component's authoring schema.
pub(super) fn environment_schema() -> ComponentSchema {
    ComponentSchema {
        kind: component_type::ENVIRONMENT.to_owned(),
        label: "Environment".to_owned(),
        description: "The level's sky, sun and moon clock, and wind; the first entity carrying one sets the level."
            .to_owned(),
        category: component_category::LIGHTING.to_owned(),
        fields: vec![
            ComponentFieldSchema {
                label: text("clock and wind"),
                description: text(
                    "timeOfDay (0 to 1, 0.5 is noon), dayLengthSeconds (0 stops the clock), latitudeDegrees, dayOfYear, \
                     northRadians, timeScale, and wind (speed, directionRadians, gustStrength, gustPeriodSeconds).",
                ),
                default: Some(json!({})),
                ..ComponentFieldSchema::new(SIMULATION_FIELD, ComponentFieldKind::Record)
            },
            ComponentFieldSchema {
                description: text(
                    "atmosphere (rayleigh, mie, ozone, density, fogDensity), clouds (coverage, density, altitude, \
                     thickness, shadowStrength), lightShafts, and ambientOcclusion.",
                ),
                default: Some(json!({})),
                ..ComponentFieldSchema::new(SKY_FIELD, ComponentFieldKind::Record)
            },
        ],
        presets: vec![
            preset(component_preset::ENVIRONMENT_DEFAULT, "Default", json!({"simulation": {}, "sky": {}})),
            preset(
                component_preset::ENVIRONMENT_DUSK,
                "Dusk",
                json!({
                    "simulation": {"timeOfDay": 0.76, "dayLengthSeconds": 0, "wind": {"speed": 6}},
                    "sky": {"clouds": {"coverage": 0.4}}
                }),
            ),
        ],
        requires: Vec::new(),
    }
}

#[cfg(test)]
#[path = "environment.test.rs"]
mod tests;
