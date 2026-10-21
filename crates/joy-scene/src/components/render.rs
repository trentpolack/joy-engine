// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The `render` component: a primitive or FORM mesh drawn at the entity transform.

use joy_core::math::Vec3;
use joy_core::{
    JsonRecord,
    LinearColor,
    PrimitiveShape,
};
use joy_gameplay::{
    AssetPathError,
    ComponentFieldKind,
    ComponentFieldSchema,
    ComponentSchema,
    asset_path,
    component_asset_kind,
    component_category,
    component_preset,
    component_type,
    read_vector,
    vector_to_json,
};
use serde_json::{
    Value,
    json,
};

use super::{
    ComponentError,
    names_asset,
    preset,
    strings,
    text,
};

/// File extensions accepted for `render.asset`.
const MESH_EXTENSIONS: [&str; 2] = [".form", ".formlab"];

/// Values assumed for omitted `render` fields. Size is full dimensions in meters before entity scale; colors are linear
/// and documents store them as `[red, green, blue]`. FORM meshes keep their authored colors (a white tint) unless a color
/// is supplied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderComponentDefaults {
    /// Full primitive dimensions in meters.
    pub size: Vec3,
    /// Tint for primitives without a color.
    pub primitive_color: LinearColor,
    /// Tint for meshes without a color.
    pub mesh_tint: LinearColor,
}

/// Engine defaults for `render` components.
pub const RENDER_COMPONENT_DEFAULTS: RenderComponentDefaults = RenderComponentDefaults {
    size: Vec3::ONE,
    primitive_color: LinearColor::rgb(0.95, 0.36, 0.12),
    mesh_tint: LinearColor::WHITE,
};

/// What a `render` component draws.
#[derive(Clone, Debug, PartialEq)]
pub enum RenderSource {
    /// A compiled FORM or FORM LAB mesh.
    Mesh {
        /// Validated `.form` or `.formlab` asset path.
        asset: String,
        /// FORM parameter overrides; omitted parameters use the script defaults.
        parameters: JsonRecord,
        /// A FORM LAB document's named variant, applied beneath `parameters`.
        variant: Option<String>,
    },
    /// A unit primitive scaled by `size` (only boxes and spheres are drawable).
    Primitive(PrimitiveShape),
    /// Nothing drawable: no asset and no supported shape. The record is preserved but invisible.
    Hidden,
}

/// Interpreted `render` component.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderComponent {
    /// What to draw.
    pub source: RenderSource,
    /// Full dimensions in meters before entity scale; invalid or omitted sizes use the default.
    pub size: Vec3,
    /// Linear tint; invalid or omitted colors use the mesh tint or primitive color default.
    pub color: LinearColor,
}

impl RenderComponent {
    /// Interpret an authored `render` record.
    ///
    /// An assigned `asset` (any value but absent, null, or an empty path) must be a safe `.form`/`.formlab` path, its
    /// `parameters`, when present, an object, and its `variant`, when present, text; these are the only failures.
    /// Malformed `size` and `color` fall back to defaults, and an unsupported shape makes the component hidden rather
    /// than invalid.
    pub fn read(record: &JsonRecord) -> Result<Self, ComponentError> {
        let source = if names_asset(record.get("asset")) {
            let Some(Value::String(path)) = record.get("asset") else {
                return Err(ComponentError::AssetPath(AssetPathError::OutsideAssets));
            };
            let asset = asset_path(path, &MESH_EXTENSIONS)?.to_owned();
            let parameters = match record.get("parameters") {
                None => JsonRecord::new(),
                Some(Value::Object(parameters)) => parameters.clone(),
                Some(_) => return Err(ComponentError::InvalidParameters),
            };
            let variant = match record.get("variant") {
                None | Some(Value::Null) => None,
                Some(Value::String(name)) if name.trim().is_empty() => None,
                Some(Value::String(name)) => Some(name.trim().to_owned()),
                Some(_) => return Err(ComponentError::InvalidVariant),
            };
            RenderSource::Mesh {
                asset,
                parameters,
                variant,
            }
        } else {
            match record.get("shape").and_then(Value::as_str).and_then(PrimitiveShape::parse) {
                Some(shape @ (PrimitiveShape::Box | PrimitiveShape::Sphere)) => RenderSource::Primitive(shape),
                _ => RenderSource::Hidden,
            }
        };

        let default_color = match source {
            RenderSource::Mesh {
                ..
            } => RENDER_COMPONENT_DEFAULTS.mesh_tint,
            _ => RENDER_COMPONENT_DEFAULTS.primitive_color,
        };
        let size = record.get("size").and_then(read_vector).unwrap_or(RENDER_COMPONENT_DEFAULTS.size);
        let color = record
            .get("color")
            .and_then(read_vector)
            .map_or(default_color, |color| LinearColor::from_rgb_array(color.to_array()));
        Ok(Self {
            source,
            size,
            color,
        })
    }
}

/// Primitive or FORM mesh presentation for `LevelScene` and `AssetScene`.
pub(super) fn render_schema() -> ComponentSchema {
    // Render primitives cannot be planes; colliders can.
    let render_shapes = [PrimitiveShape::Box, PrimitiveShape::Sphere];
    ComponentSchema {
        kind: component_type::RENDER.to_owned(),
        label: "Render".to_owned(),
        description: "Draws a primitive shape, or a FORM mesh when an asset is assigned. Entity scale multiplies size."
            .to_owned(),
        category: component_category::RENDERING.to_owned(),
        fields: vec![
            ComponentFieldSchema {
                label: text("FORM mesh"),
                description: text("Replaces the primitive shape with a compiled FORM or FORM LAB mesh."),
                asset_kinds: strings(&[component_asset_kind::FORM, component_asset_kind::FORM_LAB]),
                ..ComponentFieldSchema::new("asset", ComponentFieldKind::Asset)
            },
            ComponentFieldSchema {
                label: text("Variant"),
                description: text("A FORM LAB document's named variant, applied beneath the parameter overrides."),
                ..ComponentFieldSchema::new("variant", ComponentFieldKind::Text)
            },
            ComponentFieldSchema {
                description: text("FORM parameter overrides; omitted parameters use the script defaults."),
                ..ComponentFieldSchema::new("parameters", ComponentFieldKind::Record)
            },
            ComponentFieldSchema {
                choices: render_shapes.iter().map(|shape| shape.as_str().to_owned()).collect(),
                description: text("Primitive drawn when no FORM mesh is assigned."),
                ..ComponentFieldSchema::new("shape", ComponentFieldKind::Choice)
            },
            ComponentFieldSchema {
                unit: text("m"),
                step: Some(0.25),
                default: Some(vector_to_json(RENDER_COMPONENT_DEFAULTS.size)),
                description: text("Full primitive dimensions before entity scale."),
                ..ComponentFieldSchema::new("size", ComponentFieldKind::Vector3)
            },
            ComponentFieldSchema {
                min: Some(0.0),
                description: text(
                    "Linear RGB tint. Omitted, primitives draw orange and FORM meshes keep their own colors.",
                ),
                ..ComponentFieldSchema::new("color", ComponentFieldKind::Color)
            },
        ],
        presets: vec![
            preset(
                component_preset::RENDER_PRIMITIVE,
                "Primitive",
                json!({"shape": PrimitiveShape::Box.as_str(), "size": [1, 1, 1], "color": [0.93, 0.42, 0.22]}),
            ),
            preset(component_preset::RENDER_MESH, "FORM Mesh", json!({"asset": "", "parameters": {}})),
        ],
        requires: Vec::new(),
    }
}
