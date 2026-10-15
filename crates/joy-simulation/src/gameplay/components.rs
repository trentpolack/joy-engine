// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Built-in component vocabulary, defaults, and typed readers for presentation components.
//!
//! Projects may author any component type; the engine preserves unknown records as data and interprets only the types
//! named here. Defaults match the TypeScript `built-in-component-schemas.ts`, and the readers apply the same rules the
//! TypeScript `LevelScene` and `AssetScene` use. The full editor schema catalog is not part of the simulation layer.

use glam::Vec3;
use joy_foundation::{JsonRecord, PrimitiveShape, Rgb};
use serde_json::Value;

use super::asset_path::asset_path;
use super::definition::read_vector;
use super::error::ComponentError;

/// Authored component types interpreted by engine runtimes.
pub mod component_type {
    /// Primitive or FORM mesh presentation.
    pub const RENDER: &str = "render";
    /// World-space point light at the entity position.
    pub const LIGHT: &str = "light";
    /// Rigid body created by `LevelRuntime`.
    pub const PHYSICS: &str = "physics";
    /// Raycast vehicle attached to the entity's physics body.
    pub const VEHICLE: &str = "vehicle";
    /// Particle effect attached to the entity transform.
    pub const EFFECT: &str = "effect";
}

/// Built-in entity types registered by every [`EntityRegistry`](super::EntityRegistry).
pub mod entity_type {
    /// Plain entity; the default type of a definition.
    pub const ENTITY: &str = "entity";
    /// Controllable character.
    pub const CHARACTER: &str = "character";
    /// Character intended for player control (AI may also possess it).
    pub const PLAYER_CHARACTER: &str = "player-character";
}

/// File extensions accepted for `render.asset`.
const MESH_EXTENSIONS: [&str; 2] = [".form", ".formlab"];

/// Values assumed for omitted `render` fields. Size is full dimensions in meters before entity scale; colors are linear
/// RGB. FORM meshes keep their authored colors (a white tint) unless a color is supplied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderComponentDefaults {
    /// Full primitive dimensions in meters.
    pub size: Vec3,
    /// Tint for primitives without a color.
    pub primitive_color: Rgb,
    /// Tint for meshes without a color.
    pub mesh_tint: Rgb,
}

/// Engine defaults for `render` components.
pub const RENDER_COMPONENT_DEFAULTS: RenderComponentDefaults =
    RenderComponentDefaults { size: Vec3::ONE, primitive_color: [0.95, 0.36, 0.12], mesh_tint: [1.0, 1.0, 1.0] };

/// Values assumed for omitted `light` fields. Range is meters; intensity scales linear color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightComponentDefaults {
    /// Whether the light contributes.
    pub enabled: bool,
    /// Linear RGB color before intensity.
    pub color: Rgb,
    /// Linear intensity multiplier.
    pub intensity: f32,
    /// Falloff range in meters.
    pub range: f32,
    /// Whether the light casts shadows.
    pub casts_shadow: bool,
}

/// Engine defaults for `light` components.
pub const LIGHT_COMPONENT_DEFAULTS: LightComponentDefaults =
    LightComponentDefaults { enabled: true, color: [1.0, 1.0, 1.0], intensity: 1.0, range: 10.0, casts_shadow: true };

/// Values assumed for omitted `effect` fields. The seed selects a deterministic cosmetic sequence.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectComponentDefaults {
    /// Whether the effect plays.
    pub enabled: bool,
    /// Whether the effect restarts once every particle has expired (serialized as `loop`).
    pub looping: bool,
    /// Cosmetic random seed.
    pub seed: u32,
}

/// Engine defaults for `effect` components.
pub const EFFECT_COMPONENT_DEFAULTS: EffectComponentDefaults =
    EffectComponentDefaults { enabled: true, looping: false, seed: 7777 };

// ---------------------------------------------------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------------------------------------------------

/// What a `render` component draws.
#[derive(Clone, Debug, PartialEq)]
pub enum RenderSource {
    /// A compiled FORM or FORM LAB mesh.
    Mesh {
        /// Validated `.form` or `.formlab` asset path.
        asset: String,
        /// FORM parameter overrides; omitted parameters use the script defaults.
        parameters: JsonRecord,
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
    /// Linear RGB tint; invalid or omitted colors use the mesh tint or primitive color default.
    pub color: Rgb,
}

impl RenderComponent {
    /// Interpret an authored `render` record.
    ///
    /// A truthy `asset` must be a safe `.form`/`.formlab` path and its `parameters`, when present, an object; these are
    /// the only failures. Like the TypeScript scenes, malformed `size` and `color` fall back to defaults, and an
    /// unsupported shape makes the component hidden rather than invalid.
    pub fn read(record: &JsonRecord) -> Result<Self, ComponentError> {
        let source = match record.get("asset") {
            Some(value) if is_truthy(value) => {
                let Value::String(path) = value else {
                    return Err(ComponentError::AssetPath(super::error::AssetPathError::OutsideAssets));
                };
                let asset = asset_path(path, &MESH_EXTENSIONS)?.to_owned();
                let parameters = match record.get("parameters") {
                    None => JsonRecord::new(),
                    Some(Value::Object(parameters)) => parameters.clone(),
                    Some(_) => return Err(ComponentError::InvalidParameters),
                };
                RenderSource::Mesh { asset, parameters }
            }
            _ => match record.get("shape").and_then(Value::as_str).and_then(PrimitiveShape::parse) {
                Some(shape @ (PrimitiveShape::Box | PrimitiveShape::Sphere)) => RenderSource::Primitive(shape),
                _ => RenderSource::Hidden,
            },
        };

        let default_color = match source {
            RenderSource::Mesh { .. } => RENDER_COMPONENT_DEFAULTS.mesh_tint,
            _ => RENDER_COMPONENT_DEFAULTS.primitive_color,
        };
        let size = record.get("size").and_then(read_vector).unwrap_or(RENDER_COMPONENT_DEFAULTS.size);
        let color = record.get("color").and_then(read_vector).map_or(default_color, |color| color.to_array());
        Ok(Self { source, size, color })
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Light
// ---------------------------------------------------------------------------------------------------------------------

/// Interpreted `light` component: a point light at the entity position (rotation and scale are ignored).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightComponent {
    /// Whether the light contributes; false only when `enabled` is exactly `false`.
    pub enabled: bool,
    /// Linear RGB color before intensity.
    pub color: Rgb,
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
        let color =
            record.get("color").and_then(read_vector).map_or(LIGHT_COMPONENT_DEFAULTS.color, |color| color.to_array());
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
        if intensity < 0.0 || range <= 0.0 || color.iter().any(|channel| *channel < 0.0) {
            return Err(ComponentError::InvalidLight);
        }
        Ok(Self { enabled, color, intensity, range, casts_shadow })
    }

    /// Linear RGB radiance: color multiplied by intensity.
    pub fn radiance(&self) -> Rgb {
        self.color.map(|channel| channel * self.intensity)
    }
}

/// Read an optional finite light scalar; `None` means present but not a finite number.
fn light_scalar(value: Option<&Value>, fallback: f32) -> Option<f32> {
    match value {
        None => Some(fallback),
        Some(value) => value.as_f64().map(|number| number as f32).filter(|number| number.is_finite()),
    }
}

/// JavaScript truthiness for JSON values.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(value: Value) -> JsonRecord {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn render_components_follow_scene_rules() {
        let primitive = RenderComponent::read(&record(json!({"shape": "sphere", "size": [2, 1, 2]}))).unwrap();
        assert_eq!(primitive.source, RenderSource::Primitive(PrimitiveShape::Sphere));
        assert_eq!(primitive.size, Vec3::new(2.0, 1.0, 2.0));
        assert_eq!(primitive.color, RENDER_COMPONENT_DEFAULTS.primitive_color);

        let mesh = RenderComponent::read(&record(json!({"asset": "assets/kart.form", "size": "big"}))).unwrap();
        assert!(matches!(mesh.source, RenderSource::Mesh { ref asset, .. } if asset == "assets/kart.form"));
        assert_eq!(mesh.size, RENDER_COMPONENT_DEFAULTS.size);
        assert_eq!(mesh.color, RENDER_COMPONENT_DEFAULTS.mesh_tint);

        assert_eq!(RenderComponent::read(&record(json!({"shape": "plane"}))).unwrap().source, RenderSource::Hidden);
        assert!(RenderComponent::read(&record(json!({"asset": "assets/kart.png"}))).is_err());
        assert_eq!(
            RenderComponent::read(&record(json!({"asset": "assets/a.form", "parameters": 4}))),
            Err(ComponentError::InvalidParameters)
        );
    }

    #[test]
    fn light_components_validate_only_when_enabled() {
        let light = LightComponent::read(&record(json!({"intensity": 4, "castsShadow": false}))).unwrap();
        assert_eq!(light.radiance(), [4.0, 4.0, 4.0]);
        assert!(!light.casts_shadow);
        assert_eq!(LightComponent::read(&record(json!({"range": 0}))), Err(ComponentError::InvalidLight));
        assert_eq!(LightComponent::read(&record(json!({"intensity": null}))), Err(ComponentError::NonFiniteLightValue));
        let disabled = LightComponent::read(&record(json!({"enabled": false, "range": -1}))).unwrap();
        assert!(!disabled.enabled);
    }
}
