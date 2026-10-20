// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The `postProcess` component: post-processing settings an entity applies while the view point is inside its volume,
//! or everywhere when the volume is unbounded.
//!
//! A record holds volume fields (`enabled`, `unbounded`, `priority`, `weight`, `blendDistance`, `shape`, `halfExtents`,
//! and `radius`) beside sparse post-processing settings keyed by their control names (`exposure`, `bloomStrength`, and
//! so on); settings a volume omits inherit from lower-priority volumes and then the engine defaults. The volume shape
//! is in entity-local space: entity scale multiplies box half extents per axis and a sphere's radius by the largest
//! scale axis. [`PostProcessVolumes`](crate::PostProcessVolumes) resolves the volumes of a level at a view point.

use joy_core::JsonRecord;
use joy_core::math::Vec3;
use joy_gameplay::{
    ComponentFieldCondition,
    ComponentFieldKind,
    ComponentFieldSchema,
    ComponentSchema,
    component_category,
    component_preset,
    component_type,
    number_to_json,
    read_vector,
    vector_to_json,
};
use joy_renderer::PostProcessOverrides;
use joy_renderer::post_process::{
    POST_PROCESS_CONTROLS,
    PostProcessControl,
    PostProcessControlDefault,
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

/// Volume fields; every other key in a `postProcess` record is a post-processing setting.
const VOLUME_FIELDS: [&str; 8] =
    ["enabled", "unbounded", "priority", "weight", "blendDistance", "shape", "halfExtents", "radius"];

/// Spelling of the box volume shape.
const BOX_SHAPE: &str = "box";

/// Spelling of the sphere volume shape.
const SPHERE_SHAPE: &str = "sphere";

/// Values assumed for omitted `postProcess` volume fields. Distances are meters in entity-local space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PostProcessComponentDefaults {
    /// Whether the volume contributes.
    pub enabled: bool,
    /// Whether the volume applies everywhere instead of only inside its shape.
    pub unbounded: bool,
    /// Blend order: higher priorities apply after (on top of) lower ones.
    pub priority: f32,
    /// Strength of the volume at full coverage, in `[0, 1]`.
    pub weight: f32,
    /// Distance inside the surface over which the volume fades in; zero switches at the surface.
    pub blend_distance: f32,
    /// Half extents of a box volume.
    pub half_extents: Vec3,
    /// Radius of a sphere volume.
    pub radius: f32,
}

/// Engine defaults for `postProcess` components.
pub const POST_PROCESS_COMPONENT_DEFAULTS: PostProcessComponentDefaults = PostProcessComponentDefaults {
    enabled: true,
    unbounded: false,
    priority: 0.0,
    weight: 1.0,
    blend_distance: 0.0,
    half_extents: Vec3::splat(0.5),
    radius: 0.5,
};

/// The region a bounded post-process volume covers, in entity-local space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PostProcessShape {
    /// An axis-aligned (in local space) box.
    Box {
        /// Positive half extents in meters before entity scale.
        half_extents: Vec3,
    },
    /// A sphere around the entity origin.
    Sphere {
        /// Positive radius in meters before entity scale.
        radius: f32,
    },
}

/// Interpreted `postProcess` component.
#[derive(Clone, Debug, PartialEq)]
pub struct PostProcessComponent {
    /// Whether the volume contributes; false only when `enabled` is `false`.
    pub enabled: bool,
    /// Whether the volume applies everywhere; its shape is then unused.
    pub unbounded: bool,
    /// Blend order: higher priorities apply after lower ones; equal priorities keep authored entity order.
    pub priority: f32,
    /// Strength at full coverage, in `[0, 1]`.
    pub weight: f32,
    /// Fade distance inside the surface in meters; nonnegative.
    pub blend_distance: f32,
    /// Covered region of a bounded volume.
    pub shape: PostProcessShape,
    /// The settings this volume applies.
    pub overrides: PostProcessOverrides,
}

impl PostProcessComponent {
    /// Interpret an authored `postProcess` record.
    ///
    /// Volume fields must have their documented types and ranges, and every other key must be a valid
    /// post-processing setting; anything else fails so a typo never silently does nothing.
    pub fn read(record: &JsonRecord) -> Result<Self, ComponentError> {
        let defaults = POST_PROCESS_COMPONENT_DEFAULTS;
        let enabled = boolean(record, "enabled", defaults.enabled)?;
        let unbounded = boolean(record, "unbounded", defaults.unbounded)?;
        let priority = number(record, "priority", defaults.priority, |_| true, "must be a finite number")?;
        let weight = number(record, "weight", defaults.weight, |value| (0.0..=1.0).contains(&value), "must be in 0–1")?;
        let blend_distance =
            number(record, "blendDistance", defaults.blend_distance, |value| value >= 0.0, "cannot be negative")?;

        let shape = match record.get("shape") {
            None => BOX_SHAPE,
            Some(Value::String(shape)) if shape == BOX_SHAPE || shape == SPHERE_SHAPE => shape.as_str(),
            Some(_) => {
                return Err(ComponentError::InvalidPostProcessField {
                    field: "shape",
                    requirement: "must be box or sphere",
                });
            }
        };
        let shape = if shape == SPHERE_SHAPE {
            let radius = number(record, "radius", defaults.radius, |value| value > 0.0, "must be positive")?;
            PostProcessShape::Sphere {
                radius,
            }
        } else {
            let half_extents = match record.get("halfExtents") {
                None => defaults.half_extents,
                Some(value) => read_vector(value).filter(|extents| extents.min_element() > 0.0).ok_or(
                    ComponentError::InvalidPostProcessField {
                        field: "halfExtents",
                        requirement: "must be three positive numbers",
                    },
                )?,
            };
            PostProcessShape::Box {
                half_extents,
            }
        };

        let mut overrides = PostProcessOverrides::new();
        for (key, value) in record.iter().filter(|(key, _)| !VOLUME_FIELDS.contains(&key.as_str())) {
            overrides.set(key, value.clone())?;
        }
        Ok(Self {
            enabled,
            unbounded,
            priority,
            weight,
            blend_distance,
            shape,
            overrides,
        })
    }
}

/// Post-processing volume read by `PostProcessVolumes`.
pub(super) fn post_process_schema() -> ComponentSchema {
    let defaults = POST_PROCESS_COMPONENT_DEFAULTS;
    let mut fields = vec![
        ComponentFieldSchema {
            default: Some(Value::Bool(defaults.enabled)),
            ..ComponentFieldSchema::new("enabled", ComponentFieldKind::Boolean)
        },
        ComponentFieldSchema {
            default: Some(Value::Bool(defaults.unbounded)),
            description: text("Apply everywhere instead of only while the view is inside the volume's shape."),
            ..ComponentFieldSchema::new("unbounded", ComponentFieldKind::Boolean)
        },
        ComponentFieldSchema {
            step: Some(1.0),
            default: Some(number_to_json(defaults.priority)),
            description: text("Higher priorities apply on top of lower ones; equal priorities keep entity order."),
            ..ComponentFieldSchema::new("priority", ComponentFieldKind::Number)
        },
        ComponentFieldSchema {
            min: Some(0.0),
            max: Some(1.0),
            step: Some(0.05),
            default: Some(number_to_json(defaults.weight)),
            description: text("Strength at full coverage."),
            ..ComponentFieldSchema::new("weight", ComponentFieldKind::Number)
        },
        ComponentFieldSchema {
            label: text("blend distance"),
            unit: text("m"),
            min: Some(0.0),
            step: Some(0.25),
            default: Some(number_to_json(defaults.blend_distance)),
            description: text("Distance inside the surface over which the volume fades in; zero switches at the edge."),
            ..ComponentFieldSchema::new("blendDistance", ComponentFieldKind::Number)
        },
        ComponentFieldSchema {
            choices: vec![BOX_SHAPE.to_owned(), SPHERE_SHAPE.to_owned()],
            default: Some(Value::from(BOX_SHAPE)),
            description: text("Covered region of a bounded volume, in entity-local space."),
            ..ComponentFieldSchema::new("shape", ComponentFieldKind::Choice)
        },
        ComponentFieldSchema {
            label: text("half extents"),
            unit: text("m"),
            min: Some(0.0),
            min_exclusive: true,
            step: Some(0.25),
            default: Some(vector_to_json(defaults.half_extents)),
            description: text("Half the box size before entity scale."),
            when: Some(shape_condition(BOX_SHAPE)),
            ..ComponentFieldSchema::new("halfExtents", ComponentFieldKind::Vector3)
        },
        ComponentFieldSchema {
            unit: text("m"),
            min: Some(0.0),
            min_exclusive: true,
            step: Some(0.25),
            default: Some(number_to_json(defaults.radius)),
            description: text("Sphere radius before entity scale (the largest scale axis applies)."),
            when: Some(shape_condition(SPHERE_SHAPE)),
            ..ComponentFieldSchema::new("radius", ComponentFieldKind::Number)
        },
    ];
    fields.extend(POST_PROCESS_CONTROLS.iter().map(setting_field));

    ComponentSchema {
        kind: component_type::POST_PROCESS.to_owned(),
        label: "Post-Processing".to_owned(),
        description:
            "Applies post-processing settings while the view is inside the volume, or everywhere when unbounded."
                .to_owned(),
        category: component_category::RENDERING.to_owned(),
        fields,
        presets: vec![
            preset(component_preset::POST_PROCESS_GLOBAL, "Global", json!({"unbounded": true})),
            preset(
                component_preset::POST_PROCESS_BOX,
                "Box Volume",
                json!({"shape": BOX_SHAPE, "halfExtents": [5, 3, 5], "priority": 10, "blendDistance": 1}),
            ),
            preset(
                component_preset::POST_PROCESS_SPHERE,
                "Sphere Volume",
                json!({"shape": SPHERE_SHAPE, "radius": 5, "priority": 10, "blendDistance": 1}),
            ),
        ],
        requires: Vec::new(),
    }
}

/// One post-processing setting as an optional component field; omitted, it inherits.
fn setting_field(control: &PostProcessControl) -> ComponentFieldSchema {
    // Display-group labels are specific already ("Exposure"); others repeat across groups ("Strength").
    let label = if control.group == "Display" {
        control.label.to_owned()
    } else {
        format!("{} {}", control.group, control.label)
    };
    let kind = match control.default {
        PostProcessControlDefault::Number(_) => ComponentFieldKind::Number,
        PostProcessControlDefault::Choice(_) => ComponentFieldKind::Choice,
    };
    ComponentFieldSchema {
        label: Some(label),
        description: Some(format!("{} Omitted, it inherits.", control.description)),
        min: control.minimum,
        min_exclusive: control.exclusive_minimum,
        max: control.maximum,
        step: control.step,
        choices: control.choices.iter().map(|choice| (*choice).to_owned()).collect(),
        ..ComponentFieldSchema::new(control.key, kind)
    }
}

/// Activate a field only for one volume shape.
fn shape_condition(shape: &str) -> ComponentFieldCondition {
    ComponentFieldCondition {
        field: "shape".to_owned(),
        equals: vec![shape.to_owned()],
    }
}

/// An optional boolean volume field.
fn boolean(record: &JsonRecord, field: &'static str, fallback: bool) -> Result<bool, ComponentError> {
    match record.get(field) {
        None => Ok(fallback),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(ComponentError::InvalidPostProcessField {
            field,
            requirement: "must be true or false",
        }),
    }
}

/// An optional finite number volume field that also passes `valid`.
fn number(
    record: &JsonRecord,
    field: &'static str,
    fallback: f32,
    valid: impl Fn(f32) -> bool,
    requirement: &'static str,
) -> Result<f32, ComponentError> {
    let Some(value) = record.get(field) else {
        return Ok(fallback);
    };
    value.as_f64().map(|value| value as f32).filter(|value| value.is_finite() && valid(*value)).ok_or(
        ComponentError::InvalidPostProcessField {
            field,
            requirement,
        },
    )
}
