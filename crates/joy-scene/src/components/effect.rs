// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The `effect` component: a JOYFX particle effect attached to the entity transform, played by `AssetScene`.

use joy_gameplay::{
    ComponentFieldKind,
    ComponentFieldSchema,
    ComponentSchema,
    component_asset_kind,
    component_category,
    component_preset,
    component_type,
};
use serde_json::{
    Value,
    json,
};

use super::{
    preset,
    strings,
    text,
};

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
pub const EFFECT_COMPONENT_DEFAULTS: EffectComponentDefaults = EffectComponentDefaults {
    enabled: true,
    looping: false,
    seed: 7777,
};

/// Particle effect simulated per entity by `AssetScene`.
pub(super) fn effect_schema() -> ComponentSchema {
    ComponentSchema {
        kind: component_type::EFFECT.to_owned(),
        label: "Particle Effect".to_owned(),
        description: "Plays a JOYFX effect attached to the entity transform.".to_owned(),
        category: component_category::EFFECTS.to_owned(),
        fields: vec![
            ComponentFieldSchema {
                label: text("JOYFX effect"),
                required: true,
                asset_kinds: strings(&[component_asset_kind::PARTICLE_EFFECT]),
                ..ComponentFieldSchema::new("asset", ComponentFieldKind::Asset)
            },
            ComponentFieldSchema {
                default: Some(Value::Bool(EFFECT_COMPONENT_DEFAULTS.enabled)),
                ..ComponentFieldSchema::new("enabled", ComponentFieldKind::Boolean)
            },
            ComponentFieldSchema {
                default: Some(Value::Bool(EFFECT_COMPONENT_DEFAULTS.looping)),
                description: text("Restart once every particle has expired."),
                ..ComponentFieldSchema::new("loop", ComponentFieldKind::Boolean)
            },
            ComponentFieldSchema {
                integer: true,
                step: Some(1.0),
                default: Some(Value::from(EFFECT_COMPONENT_DEFAULTS.seed)),
                ..ComponentFieldSchema::new("seed", ComponentFieldKind::Number)
            },
            ComponentFieldSchema {
                description: text("Effect parameter overrides; omitted parameters use the effect defaults."),
                ..ComponentFieldSchema::new("parameters", ComponentFieldKind::Record)
            },
        ],
        presets: vec![preset(
            component_preset::EFFECT_PARTICLES,
            "JOYFX Effect",
            json!({"asset": "", "enabled": true, "loop": true, "seed": EFFECT_COMPONENT_DEFAULTS.seed, "parameters": {}}),
        )],
        requires: Vec::new(),
    }
}
