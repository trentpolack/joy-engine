// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Schemas for the simulation components this crate interprets (`physics` and `vehicle`); `joy-scene` defines the
//! presentation components' schemas and assembles the engine's full catalog.
//!
//! Field defaults come from the same constants the runtime readers use (the physics body, vehicle, and wheel defaults),
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
use joy_core::{
    JsonRecord,
    PrimitiveShape,
};
use joy_physics::{
    BODY_DEFAULTS,
    VEHICLE_DEFAULTS,
    WHEEL_DEFAULTS,
};

/// Largest authored steering angle in radians (`Math.PI / 2`).
const MAX_STEER_RADIANS: f64 = std::f64::consts::FRAC_PI_2;

/// The simulation components' schemas in authoring display order: physics, then vehicle.
///
/// Each call returns fresh, caller-owned schemas.
pub fn built_in_component_schemas() -> Vec<ComponentSchema> {
    vec![physics_schema(), vehicle_schema()]
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

/// One raycast wheel, in chassis-local meters.
fn wheel_fields() -> Vec<ComponentFieldSchema> {
    vec![
        ComponentFieldSchema {
            unit: text("m"),
            step: Some(0.05),
            required: true,
            ..field("position", ComponentFieldKind::Vector3)
        },
        ComponentFieldSchema {
            unit: text("m"),
            min: Some(0.0),
            min_exclusive: true,
            step: Some(0.05),
            default: Some(number_to_json(WHEEL_DEFAULTS.radius)),
            ..field("radius", ComponentFieldKind::Number)
        },
        ComponentFieldSchema {
            default: Some(Value::Bool(WHEEL_DEFAULTS.steering)),
            ..field("steering", ComponentFieldKind::Boolean)
        },
        ComponentFieldSchema {
            default: Some(Value::Bool(WHEEL_DEFAULTS.driven)),
            ..field("driven", ComponentFieldKind::Boolean)
        },
        ComponentFieldSchema {
            default: Some(Value::Bool(WHEEL_DEFAULTS.brake)),
            ..field("brake", ComponentFieldKind::Boolean)
        },
        wheel_number(
            "suspensionRestLength",
            "suspension rest length",
            Some("m"),
            0.05,
            WHEEL_DEFAULTS.suspension_rest_length,
        ),
        wheel_number("suspensionStiffness", "suspension stiffness", None, 1.0, WHEEL_DEFAULTS.suspension_stiffness),
        wheel_number("dampingCompression", "damping compression", None, 0.1, WHEEL_DEFAULTS.damping_compression),
        wheel_number("dampingRelaxation", "damping relaxation", None, 0.1, WHEEL_DEFAULTS.damping_relaxation),
        wheel_number(
            "maxSuspensionTravel",
            "max suspension travel",
            Some("m"),
            0.05,
            WHEEL_DEFAULTS.max_suspension_travel,
        ),
        wheel_number(
            "maxSuspensionForce",
            "max suspension force",
            Some("N"),
            100.0,
            WHEEL_DEFAULTS.max_suspension_force,
        ),
        ComponentFieldSchema {
            label: text("friction slip"),
            min: Some(0.0),
            step: Some(0.1),
            description: text("Defaults to the vehicle friction slip."),
            ..field("frictionSlip", ComponentFieldKind::Number)
        },
        ComponentFieldSchema {
            label: text("roll influence"),
            min: Some(0.0),
            max: Some(1.0),
            step: Some(0.01),
            default: Some(number_to_json(WHEEL_DEFAULTS.roll_influence)),
            ..field("rollInfluence", ComponentFieldKind::Number)
        },
    ]
}

/// Raycast vehicle attached by `LevelRuntime` to the entity's physics body.
fn vehicle_schema() -> ComponentSchema {
    let wheel = |position: [f64; 3], front: bool| {
        if front {
            json!({"position": position, "steering": true, "brake": true})
        } else {
            json!({"position": position, "driven": true, "brake": true})
        }
    };
    ComponentSchema {
        kind: component_type::VEHICLE.to_owned(),
        label: "Vehicle".to_owned(),
        description: "Raycast suspension and drive on the physics body. Requires uniform entity scale.".to_owned(),
        category: component_category::PHYSICS.to_owned(),
        fields: vec![
            ComponentFieldSchema {
                required: true,
                items: Some(wheel_fields()),
                ..field("wheels", ComponentFieldKind::List)
            },
            ComponentFieldSchema {
                label: text("engine force"),
                unit: text("N"),
                min: Some(0.0),
                step: Some(10.0),
                default: Some(number_to_json(VEHICLE_DEFAULTS.engine_force)),
                ..field("engineForce", ComponentFieldKind::Number)
            },
            ComponentFieldSchema {
                label: text("max steer"),
                unit: text("rad"),
                min: Some(0.0),
                max: Some(MAX_STEER_RADIANS),
                step: Some(0.05),
                default: Some(number_to_json(VEHICLE_DEFAULTS.max_steer)),
                ..field("maxSteer", ComponentFieldKind::Number)
            },
            ComponentFieldSchema {
                label: text("brake force"),
                unit: text("N"),
                min: Some(0.0),
                step: Some(1.0),
                default: Some(number_to_json(VEHICLE_DEFAULTS.brake_force)),
                ..field("brakeForce", ComponentFieldKind::Number)
            },
            ComponentFieldSchema {
                label: text("friction slip"),
                min: Some(0.0),
                step: Some(0.1),
                default: Some(number_to_json(VEHICLE_DEFAULTS.friction_slip)),
                ..field("frictionSlip", ComponentFieldKind::Number)
            },
        ],
        presets: vec![preset(
            component_preset::VEHICLE_FOUR_WHEEL,
            "Four-Wheel Vehicle",
            json!({"wheels": [
                wheel([-0.75, -0.05, 0.75], true),
                wheel([0.75, -0.05, 0.75], true),
                wheel([-0.75, -0.05, -0.75], false),
                wheel([0.75, -0.05, -0.75], false)
            ]}),
        )],
        requires: vec![component_type::PHYSICS.to_owned()],
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------------------------------------------------

/// A field with a name and kind only.
fn field(name: &str, kind: ComponentFieldKind) -> ComponentFieldSchema {
    ComponentFieldSchema::new(name, kind)
}

/// A labeled, nonnegative wheel tuning number with a step and default.
fn wheel_number(name: &str, label: &str, unit: Option<&str>, step: f64, default: f32) -> ComponentFieldSchema {
    ComponentFieldSchema {
        label: text(label),
        unit: unit.map(str::to_owned),
        min: Some(0.0),
        step: Some(step),
        default: Some(number_to_json(default)),
        ..field(name, ComponentFieldKind::Number)
    }
}

/// Activate a field only for one collider shape.
fn shape_condition(shape: PrimitiveShape) -> ComponentFieldCondition {
    ComponentFieldCondition { field: "shape".to_owned(), equals: vec![shape.as_str().to_owned()] }
}

/// A preset whose data is a JSON object literal.
fn preset(id: &str, label: &str, data: Value) -> ComponentPreset {
    let data = match data {
        Value::Object(record) => record,
        _ => JsonRecord::new(),
    };
    ComponentPreset { id: id.to_owned(), label: label.to_owned(), data }
}

/// Optional owned text.
fn text(value: &str) -> Option<String> {
    Some(value.to_owned())
}
