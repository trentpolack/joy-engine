// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! How editors should present a parameter descriptor from [`GeometryData::parameters`](crate::GeometryData).
//!
//! Descriptors stay plain JSON (they round-trip through `.formlab` documents and browser hosts unchanged); this module
//! reads their optional presentation fields (`label`, `group`, `info`, `kind`, and `options`) so FORM LAB, Joy Editor,
//! and game tools present a script's controls the same way. Reading never fails: a descriptor without presentation
//! fields reads as a continuous control titled from its name.

use serde_json::Value as Json;

/// The control an editor should show for a numeric parameter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParameterControl {
    /// A continuous value (or vector or color components) between the declared bounds.
    Continuous,
    /// A whole number between the declared bounds.
    Integer,
    /// Zero or one, shown as a toggle.
    Toggle,
    /// An index into these option labels, shown as a choice.
    Choice(Vec<String>),
    /// A whole-number random seed, shown with a reroll control.
    Seed,
}

/// The control a descriptor asks for; descriptors without a recognized `kind` are continuous.
pub fn parameter_control(descriptor: &Json) -> ParameterControl {
    match descriptor.get("kind").and_then(Json::as_str) {
        Some("int") => ParameterControl::Integer,
        Some("bool") => ParameterControl::Toggle,
        Some("seed") => ParameterControl::Seed,
        Some("enum") => {
            let options = descriptor
                .get("options")
                .and_then(Json::as_array)
                .map(|options| options.iter().filter_map(Json::as_str).map(str::to_owned).collect())
                .unwrap_or_default();
            ParameterControl::Choice(options)
        }
        _ => ParameterControl::Continuous,
    }
}

/// The author's `label`, or the parameter name with underscores read as spaces.
pub fn parameter_label(descriptor: &Json) -> String {
    if let Some(label) = descriptor.get("label").and_then(Json::as_str) {
        return label.to_owned();
    }
    descriptor.get("name").and_then(Json::as_str).unwrap_or_default().replace('_', " ")
}

/// The author's `group`, which editors use to gather related parameters under one heading.
pub fn parameter_group(descriptor: &Json) -> Option<&str> {
    descriptor.get("group").and_then(Json::as_str)
}

/// The author's `info`: one line of help editors show on hover.
pub fn parameter_info(descriptor: &Json) -> Option<&str> {
    descriptor.get("info").and_then(Json::as_str)
}
