// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Schemas for the simulation components this crate interprets (`physics`); `joy-scene` defines the
//! presentation components' schemas and assembles the engine's full catalog.
//!
//! Field defaults come from the same constants the runtime readers use (the physics body defaults),
//! so the inspector always shows what the runtime assumes for an omitted field.

use serde_json::{
    Value,
    json,
};

use super::component_schema::{
    ComponentFieldCondition,
    ComponentFieldKind,
    ComponentFieldSchema,
    ComponentPreset,
    ComponentSchema,
};
use super::component_vocabulary::{
    component_category,
    component_preset,
    component_type,
};
use super::entity_definition::{
    number_to_json,
    vector_to_json,
};
use super::physics_component::physics_field;
use joy_core::{
    JsonRecord,
    PrimitiveShape,
};
use joy_physics::BODY_DEFAULTS;

/// The simulation components' schemas in authoring display order.
///
/// Each call returns fresh, caller-owned schemas.
pub fn built_in_component_schemas() -> Vec<ComponentSchema> {
    vec![physics_schema()]
}

/// Rigid body created by `LevelRuntime`. Collider dimensions are local; entity scale is applied once at spawn.
fn physics_schema() -> ComponentSchema {
    let physics_shapes = [PrimitiveShape::Box, PrimitiveShape::Sphere, PrimitiveShape::Plane];
    ComponentSchema {
        kind: component_type::PHYSICS.to_owned(),
        label: "Physics".to_owned(),
        description: "Creates a rigid body. Mass 0 is static; colliders are authored separately from render meshes."
            .to_owned(),
        category: component_category::PHYSICS.to_owned(),
        fields: vec![
            ComponentFieldSchema {
                choices: physics_shapes.iter().map(|shape| shape.as_str().to_owned()).collect(),
                required: true,
                ..field("shape", ComponentFieldKind::Choice)
            },
            ComponentFieldSchema {
                unit: text("kg"),
                min: Some(0.0),
                step: Some(1.0),
                default: Some(number_to_json(BODY_DEFAULTS.mass)),
                description: text("0 makes the body static; planes must be static."),
                ..field("mass", ComponentFieldKind::Number)
            },
            ComponentFieldSchema {
                label: text("half extents"),
                unit: text("m"),
                min: Some(0.0),
                min_exclusive: true,
                step: Some(0.25),
                default: Some(vector_to_json(BODY_DEFAULTS.half_extents)),
                when: Some(shape_condition(PrimitiveShape::Box)),
                ..field("halfExtents", ComponentFieldKind::Vector3)
            },
            ComponentFieldSchema {
                unit: text("m"),
                min: Some(0.0),
                min_exclusive: true,
                step: Some(0.25),
                default: Some(number_to_json(BODY_DEFAULTS.radius)),
                when: Some(shape_condition(PrimitiveShape::Sphere)),
                description: text("Spheres require uniform entity scale."),
                ..field("radius", ComponentFieldKind::Number)
            },
            ComponentFieldSchema {
                min: Some(0.0),
                step: Some(0.05),
                default: Some(number_to_json(BODY_DEFAULTS.friction)),
                ..field("friction", ComponentFieldKind::Number)
            },
            ComponentFieldSchema {
                min: Some(0.0),
                max: Some(1.0),
                step: Some(0.05),
                default: Some(number_to_json(BODY_DEFAULTS.restitution)),
                ..field("restitution", ComponentFieldKind::Number)
            },
            ComponentFieldSchema {
                label: text("linear damping"),
                min: Some(0.0),
                max: Some(1.0),
                step: Some(0.01),
                default: Some(number_to_json(BODY_DEFAULTS.linear_damping)),
                ..field("linearDamping", ComponentFieldKind::Number)
            },
            ComponentFieldSchema {
                label: text("angular damping"),
                min: Some(0.0),
                max: Some(1.0),
                step: Some(0.01),
                default: Some(number_to_json(BODY_DEFAULTS.angular_damping)),
                ..field("angularDamping", ComponentFieldKind::Number)
            },
            ComponentFieldSchema {
                default: Some(json!(BODY_DEFAULTS.sensor)),
                description: text("Reports overlaps as contact events but never collides."),
                ..field(physics_field::SENSOR, ComponentFieldKind::Boolean)
            },
            ComponentFieldSchema {
                label: text("report contacts"),
                default: Some(json!(BODY_DEFAULTS.report_contacts)),
                description: text("Reports when this body starts and stops touching others."),
                ..field(physics_field::REPORT_CONTACTS, ComponentFieldKind::Boolean)
            },
            ComponentFieldSchema {
                label: text("can sleep"),
                default: Some(json!(BODY_DEFAULTS.can_sleep)),
                description: text("Lets the body stop simulating once it comes to rest."),
                ..field(physics_field::CAN_SLEEP, ComponentFieldKind::Boolean)
            },
            ComponentFieldSchema {
                label: text("collision membership"),
                min: Some(0.0),
                max: Some(f64::from(u32::MAX)),
                step: Some(1.0),
                default: Some(json!(BODY_DEFAULTS.groups.membership)),
                description: text("Bit mask of the collision layers this body belongs to."),
                ..field(physics_field::COLLISION_MEMBERSHIP, ComponentFieldKind::Number)
            },
            ComponentFieldSchema {
                label: text("collision filter"),
                min: Some(0.0),
                max: Some(f64::from(u32::MAX)),
                step: Some(1.0),
                default: Some(json!(BODY_DEFAULTS.groups.filter)),
                description: text("Bit mask of the collision layers this body interacts with."),
                ..field(physics_field::COLLISION_FILTER, ComponentFieldKind::Number)
            },
        ],
        presets: vec![
            preset(
                component_preset::PHYSICS_BOX,
                "Box Collider",
                json!({"shape": PrimitiveShape::Box.as_str(), "mass": 0, "halfExtents": [0.5, 0.5, 0.5]}),
            ),
            preset(
                component_preset::PHYSICS_SPHERE,
                "Sphere Collider",
                json!({"shape": PrimitiveShape::Sphere.as_str(), "mass": 0, "radius": 0.5}),
            ),
        ],
        requires: Vec::new(),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------------------------------------------------

/// A field with a name and kind only.
fn field(name: &str, kind: ComponentFieldKind) -> ComponentFieldSchema {
    ComponentFieldSchema::new(name, kind)
}

/// Activate a field only for one collider shape.
fn shape_condition(shape: PrimitiveShape) -> ComponentFieldCondition {
    ComponentFieldCondition {
        field: "shape".to_owned(),
        equals: vec![shape.as_str().to_owned()],
    }
}

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
