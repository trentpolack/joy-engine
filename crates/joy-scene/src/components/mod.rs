// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Presentation components: the authored `render`, `light`, `effect`, and `postProcess` records that scenes interpret,
//! with their defaults, typed readers, and authoring schemas.
//!
//! Simulation components (`physics`, `vehicle`) belong to `joy-gameplay`, which also owns the shared vocabulary of
//! component type names, presets, and categories so a level format has one set of names. Presentation components are
//! defined here, next to the scenes that present them, so the gameplay crate never depends on rendering. Each schema's
//! field defaults come from the same constants its reader uses, so the inspector shows what the scene assumes for an
//! omitted field.

mod effect;
mod light;
mod post_process;
mod render;

pub use effect::{
    EFFECT_COMPONENT_DEFAULTS,
    EffectComponentDefaults,
};
pub use light::{
    LIGHT_COMPONENT_DEFAULTS,
    LightComponent,
    LightComponentDefaults,
};
pub use post_process::{
    POST_PROCESS_COMPONENT_DEFAULTS,
    PostProcessComponent,
    PostProcessComponentDefaults,
    PostProcessShape,
};
pub use render::{
    RENDER_COMPONENT_DEFAULTS,
    RenderComponent,
    RenderComponentDefaults,
    RenderSource,
};

use joy_core::JsonRecord;
use joy_gameplay::{
    AssetPathError,
    ComponentPreset,
    ComponentSchema,
    ComponentSchemaCatalog,
    built_in_component_schemas,
};
use joy_renderer::PostProcessError;
use serde_json::Value;

/// A presentation component record that cannot be presented.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ComponentError {
    /// A light scalar was present but not a finite number.
    #[error("Scene light values must be finite numbers.")]
    NonFiniteLightValue,
    /// Light color or intensity was negative, or range was not positive.
    #[error("Light color/intensity must be nonnegative and range must be positive.")]
    InvalidLight,
    /// Asset parameters were present but not an object.
    #[error("Asset parameters must be a record.")]
    InvalidParameters,
    /// A render variant was present but not text.
    #[error("Render variant must be text.")]
    InvalidVariant,
    /// A mesh or effect asset path was invalid.
    #[error(transparent)]
    AssetPath(#[from] AssetPathError),
    /// A post-process volume field had the wrong type or range.
    #[error("Post-process {field} {requirement}.")]
    InvalidPostProcessField {
        /// Field name.
        field: &'static str,
        /// Human-readable requirement.
        requirement: &'static str,
    },
    /// A post-process volume's setting override was invalid or unknown.
    #[error(transparent)]
    PostProcess(#[from] PostProcessError),
}

/// The presentation components' schemas in authoring display order: render, light, effect, then post-process.
///
/// Each call returns fresh, caller-owned schemas.
pub fn presentation_component_schemas() -> Vec<ComponentSchema> {
    vec![render::render_schema(), light::light_schema(), effect::effect_schema(), post_process::post_process_schema()]
}

/// A catalog of every component the engine interprets: presentation components first, then the simulation
/// components `joy-gameplay` defines. Projects register their own schemas on top.
pub fn engine_component_catalog() -> ComponentSchemaCatalog {
    let mut catalog = ComponentSchemaCatalog::empty();
    for schema in presentation_component_schemas().into_iter().chain(built_in_component_schemas()) {
        // The engine's schemas are valid and mutually unique; `engine_component_catalog_registers` tests this.
        catalog.register(schema).expect("engine component schemas are valid");
    }
    catalog
}

// ---------------------------------------------------------------------------------------------------------------------
// Schema helpers
// ---------------------------------------------------------------------------------------------------------------------

/// A preset whose data is a JSON object literal.
fn preset(id: &str, label: &str, data: Value) -> ComponentPreset {
    let data = match data {
        Value::Object(record) => record,
        _ => JsonRecord::new(),
    };
    ComponentPreset {
        id: id.to_owned(),
        label: label.to_owned(),
        data,
    }
}

/// Optional owned text.
fn text(value: &str) -> Option<String> {
    Some(value.to_owned())
}

/// Owned copies of string literals.
fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// Whether an asset field names an asset: absent, null, and empty paths assign nothing.
fn names_asset(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::String(path)) => !path.is_empty(),
        Some(_) => true,
    }
}
