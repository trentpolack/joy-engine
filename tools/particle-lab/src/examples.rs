// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Bundled example effects and emitter insertion presets. Every call returns a fully
//! detached document, so editing one never changes the bundled original.

use joy_engine::particles::{
    ParticleBurst,
    ParticleEffectAsset,
    ParticleEmitterAsset,
    ParticleRenderer,
    compile_particle_effect,
};
use joy_engine::serde_json::{
    Value,
    json,
};

use crate::constants::{
    EmitterPreset,
    example_emitter,
};
use crate::document::read_asset;

/// The orbiting point-light example, shared with the TypeScript lab.
const ORBITING_LIGHTS_SOURCE: &str = include_str!("../assets/joyfx/orbiting-lights.joyfx");

/// The textured and flipbook example with its embedded original PNGs.
const PRISM_SPRITE_GARDEN_SOURCE: &str = include_str!("../assets/joyfx/prism-sprite-garden.joyfx");

/// One bundled example: its menu label and how to build it.
#[derive(Clone, Copy, Debug)]
pub struct Example {
    /// Menu label, `Category / Name`.
    pub label: &'static str,
    build: fn() -> Value,
}

/// The bundled examples in menu order.
pub const EXAMPLES: [Example; 4] = [
    Example { label: "Impact / Ember Bloom", build: ember_impact },
    Example { label: "Continuous / Solar Swirl", build: solar_swirl },
    Example { label: "Materials / Prism Sprite Garden", build: prism_sprite_garden },
    Example { label: "Lighting / Orbiting Lights", build: orbiting_lights },
];

/// One emitter insertion recipe; recipes never borrow an effect's parameters or child emitters.
#[derive(Clone, Copy, Debug)]
pub struct PresetInfo {
    /// The preset.
    pub preset: EmitterPreset,
    /// Button label.
    pub label: &'static str,
    /// One-line description.
    pub description: &'static str,
}

/// Emitter presets in chooser order.
pub const EMITTER_PRESETS: [PresetInfo; 5] = [
    PresetInfo {
        preset: EmitterPreset::Scratch,
        label: "Scratch",
        description: "Empty scripts and no emission; build your own.",
    },
    PresetInfo {
        preset: EmitterPreset::Spray,
        label: "Soft Spray",
        description: "Continuous particles moving outward in 3D.",
    },
    PresetInfo {
        preset: EmitterPreset::Sparks,
        label: "Spark Burst",
        description: "A short streak burst with gravity.",
    },
    PresetInfo { preset: EmitterPreset::Ring, label: "Ring Flash", description: "Expanding rings for an impact." },
    PresetInfo {
        preset: EmitterPreset::Smoke,
        label: "Smoke",
        description: "Rising soft particles with a gentle fade.",
    },
];

/// The example's authored JSON, before normalization.
pub fn example_json(index: usize) -> Value {
    (EXAMPLES.get(index).unwrap_or(&EXAMPLES[0]).build)()
}

/// A detached, normalized copy of example `index` (the first example for an unknown index).
pub fn copy_example(index: usize) -> ParticleEffectAsset {
    let value = example_json(index);
    let compiled = compile_particle_effect(&value).expect("bundled examples compile");
    compiled.asset().clone()
}

/// The default authoring template: isotropic motion in XYZ, not an XY fan, adjusted by `preset`.
pub fn create_emitter_template(id: &str, preset: EmitterPreset) -> ParticleEmitterAsset {
    let mut emitter = ParticleEmitterAsset {
        id: id.to_owned(),
        enabled: true,
        root: true,
        duration: 1.0,
        rate: 20.0,
        bursts: Vec::new(),
        lifetime: 1.0,
        renderer: ParticleRenderer::Soft,
        texture: None,
        light: None,
        spawn: "let angle = rand(0, tau);\nlet vertical = rand(-1, 1);\nlet radial = sqrt(1 - vertical * vertical);\nvelocity = vector(cos(angle) * radial, vertical, sin(angle) * radial) * 80;\nsize = 3;".to_owned(),
        update: "color.a = 1 - t;".to_owned(),
        trail: None,
        death: None,
    };
    match preset {
        EmitterPreset::Spray => {}
        EmitterPreset::Scratch => {
            emitter.rate = 0.0;
            emitter.spawn.clear();
            emitter.update.clear();
        }
        EmitterPreset::Sparks => {
            emitter.duration = 0.0;
            emitter.rate = 0.0;
            emitter.renderer = ParticleRenderer::Streak;
            emitter.bursts = vec![ParticleBurst { time: 0.0, count: 40 }];
            emitter.spawn.push_str("\ncolor = rgba(2.8, 0.6, 0.08, 1);");
            emitter.update = "velocity.y-= 90*dt; color.a = 1 - t;".to_owned();
        }
        EmitterPreset::Ring => {
            emitter.duration = 0.0;
            emitter.rate = 0.0;
            emitter.renderer = ParticleRenderer::Ring;
            emitter.lifetime = 0.5;
            emitter.bursts = vec![ParticleBurst { time: 0.0, count: 3 }];
            emitter.spawn = "size = rand(6, 12); color = rgba(2.4, 0.7, 0.1, 1);".to_owned();
            emitter.update = "size+= 60*dt; color.a = 1 - t;".to_owned();
        }
        EmitterPreset::Smoke => {
            emitter.lifetime = 2.0;
            emitter.spawn =
                "velocity = vector(rand(-6, 6), rand(12, 22), rand(-6, 6)); size = rand(6, 10); color = rgba(0.4, 0.45, 0.5, 0.7);"
                    .to_owned();
            emitter.update = "size+= 8*dt; color.a = (1 - t)*0.7;".to_owned();
        }
    }
    emitter
}

fn parse_source(source: &str) -> Value {
    // Validate once through the portable reader so a broken bundled file fails loudly in tests.
    debug_assert!(read_asset(source).is_ok());
    joy_engine::serde_json::from_str(source).expect("bundled example JSON")
}

fn orbiting_lights() -> Value {
    parse_source(ORBITING_LIGHTS_SOURCE)
}

fn prism_sprite_garden() -> Value {
    parse_source(PRISM_SPRITE_GARDEN_SOURCE)
}

fn ember_impact() -> Value {
    json!({
        "version": 1,
        "name": "Ember Impact",
        "maxParticles": 2200,
        "parameters": {
            "power": { "value": 1, "min": 0.25, "max": 2 },
            "wind": { "type": "vector3", "value": [8, 0, 0], "min": [-50, -50, -50], "max": [50, 50, 50] },
            "tint": { "type": "color", "value": [1, 0.65, 0.08, 1], "min": [0, 0, 0, 0], "max": [1, 1, 1, 1] }
        },
        "emitters": [
            {
                "id": example_emitter::FLASH, "enabled": true, "root": true, "duration": 0, "rate": 0,
                "bursts": [{ "time": 0, "count": 26 }], "lifetime": 0.42, "renderer": "ring",
                "spawn": "let angle = rand(0, tau);\nsize = rand(10, 22) * power;\nposition = origin + vector(cos(angle)*rand(0, 8), sin(angle)*rand(0, 8), rand(-8, 8));\ncolor = tint;\ncolor.r*= 3;",
                "update": "size += 90 * dt;\ncolor.a = 1 - t;\ncolor.r = mix(3, 1.2, t);",
                "death": { "emitter": example_emitter::SMOKE, "count": 1 }
            },
            {
                "id": example_emitter::SPARKS, "enabled": true, "root": true, "duration": 0, "rate": 0,
                "bursts": [{ "time": 0, "count": 110 }], "lifetime": 1.25, "renderer": "streak",
                "spawn": "let angle = rand(0, tau);\nlet speed = rand(120, 360) * power;\nlet vertical = rand(-1, 1);\nlet radial = sqrt(1 - vertical * vertical);\nvelocity = vector(cos(angle)*radial, vertical, sin(angle)*radial)*speed;\nsize = rand(2, 5);\ncolor = rgba(rand(2, 4), rand(0.25, 0.8), 0.03, 1);\nlifetime = rand(0.45, 1.25);",
                "update": "velocity.y-= 190*dt;\nvelocity+= wind*dt;\nsize = mix(5, 0.5, t);\ncolor.a = 1 - smoothstep(0.55, 1, t);",
                "trail": { "emitter": example_emitter::SMOKE, "interval": 0.08, "count": 1 }
            },
            {
                "id": example_emitter::BILLOW, "enabled": true, "root": true, "duration": 0.16, "rate": 180,
                "bursts": [{ "time": 0, "count": 36 }], "lifetime": 1.15, "renderer": "billow",
                "spawn": "let angle = rand(0, tau);\nlet speed = rand(25, 95) * power;\nlet vertical = rand(-1, 1);\nlet radial = sqrt(1 - vertical * vertical);\nvelocity = vector(cos(angle)*radial, vertical, sin(angle)*radial)*speed;\nsize = rand(12, 30);\ncolor = rgba(2.4, rand(0.2, 0.55), 0.04, 1);\nlifetime = rand(0.55, 1.15);",
                "update": "drag = 2.4;\nsize += 32 * dt;\ncolor.a = (1 - t) * 0.7;\ncolor.g = mix(0.55, 0.07, t);"
            },
            {
                "id": example_emitter::SMOKE, "enabled": true, "root": false, "duration": 0, "rate": 0,
                "bursts": [], "lifetime": 1.8, "renderer": "soft",
                "spawn": "velocity = parentVelocity*0.04 + vector(rand(-9, 9), rand(8, 22), rand(-9, 9));\nsize = rand(7, 15);\ncolor = rgba(0.22, 0.16, 0.13, 1);\nlifetime = rand(0.7, 1.8);",
                "update": "drag = 1.8;\nsize += 18 * dt;\ncolor.a = (1 - t) * 0.34;"
            }
        ]
    })
}

fn solar_swirl() -> Value {
    json!({
        "version": 1,
        "name": "Solar Swirl",
        "maxParticles": 1800,
        "parameters": {
            "radius": { "value": 145, "min": 40, "max": 260 },
            "swirlSpeed": { "value": 2.2, "min": -5, "max": 5 },
            "lift": { "value": 18, "min": -40, "max": 60 }
        },
        "emitters": [
            {
                "id": example_emitter::ORBIT, "enabled": true, "root": true, "duration": 6, "rate": 100,
                "bursts": [], "lifetime": 2.4, "renderer": "soft",
                "spawn": "let angle = time * swirlSpeed + rand(-0.25, 0.25);\nlet reach = radius + rand(-18, 18);\nposition = origin + vector(cos(angle)*reach, rand(-30, 30), sin(angle)*reach);\nvelocity = vector(-sin(angle)*radius*swirlSpeed, lift, cos(angle)*radius*swirlSpeed);\nsize = rand(4, 11);\ncolor = rgba(rand(1.4, 2.8), rand(0.15, 0.65), rand(0.35, 1.2), 1);\nlifetime = rand(1.2, 2.4);",
                "update": "let pull = -1.6;\nacceleration = vector((position.x - origin.x)*pull, lift, (position.z - origin.z)*pull);\ndrag = 0.35;\ncolor.a = smoothstep(0, 0.12, t) * (1 - smoothstep(0.6, 1, t));\nsize = mix(size, 2, t);",
                "trail": { "emitter": example_emitter::DUST, "interval": 0.12, "count": 1 }
            },
            {
                "id": example_emitter::CORE, "enabled": true, "root": true, "duration": 6, "rate": 45,
                "bursts": [], "lifetime": 0.7, "renderer": "ring",
                "spawn": "let angle = rand(0, tau);\nposition = origin + vector(cos(angle)*rand(0, 20), rand(-20, 20), sin(angle)*rand(0, 20));\nsize = rand(8, 16); color = rgba(2.8, 0.45, 0.7, 1);",
                "update": "size += 34 * dt; color.a = 1 - t;"
            },
            {
                "id": example_emitter::DUST, "enabled": true, "root": false, "duration": 0, "rate": 0,
                "bursts": [], "lifetime": 0.9, "renderer": "soft",
                "spawn": "velocity = parentVelocity*0.08;\nsize = rand(3, 7); color = rgba(0.55, 0.12, 0.7, 1);",
                "update": "size += 8 * dt; color.a = (1 - t) * 0.22;"
            }
        ]
    })
}
