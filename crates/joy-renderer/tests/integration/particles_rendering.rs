// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Particle submission: frames group each effect into material batches with bounds and never touch the simulation,
//! texture sources validate, flipbooks select frames by seconds in row-major order, and texture sets stage images
//! before activation.

use joy_core::math::{
    Mat4,
    Vec3,
};
use joy_fx::{
    DEFAULT_PARTICLE_TEXTURE,
    ParticleEffect,
    ParticleEffectOptions,
    ParticleTextureDimensions,
    ParticleTextureError,
    compile_particle_effect,
    read_particle_texture_source,
};
use joy_renderer::particles::{
    ParticleFrame,
    ParticleRenderError,
    ParticleTextureSet,
};
use serde_json::{
    Value,
    json,
};

/// Largest value of a normalized 16-bit texture coordinate.
const UNORM16_MAXIMUM: f64 = 65535.0;

fn single(renderer: &str, texture: Option<Value>, spawn: &str) -> Value {
    let mut emitter = json!({"id": "one", "enabled": true, "root": true, "duration": 0, "rate": 0,
        "bursts": [{"time": 0, "count": 1}], "lifetime": 2, "renderer": renderer, "spawn": spawn, "update": ""});
    if let Some(texture) = texture {
        emitter["texture"] = texture;
    }
    json!({"version": 1, "name": "Materials", "maxParticles": 16, "parameters": {}, "emitters": [emitter]})
}

fn runtime(document: &Value) -> ParticleEffect {
    let definition = compile_particle_effect(document).unwrap();
    ParticleEffect::new(
        &definition,
        ParticleEffectOptions {
            position: [10.0, 20.0, 30.0],
            ..Default::default()
        },
    )
    .unwrap()
}

fn texture(options: Value) -> Value {
    let mut texture = json!({"source": DEFAULT_PARTICLE_TEXTURE});
    if let (Value::Object(texture), Value::Object(options)) = (&mut texture, options) {
        texture.extend(options);
    }
    texture
}

/// A prepared and activated texture set for `effect`'s definition.
fn prepared(effect: &ParticleEffect) -> ParticleTextureSet {
    let mut set = ParticleTextureSet::new();
    set.prepare(effect.definition()).unwrap();
    set.activate(Some(effect.definition()));
    set
}

#[test]
fn frames_group_effects_into_material_batches_without_touching_the_simulation() {
    // Sprites, straight and premultiplied squares, an additive streak, and a second sprite emitter.
    let mut document = single("textured", Some(texture(json!({}))), "size = 2; alpha = 0.5;");
    let extra = |id: &str, renderer: &str, blend: &str, spawn: &str| {
        let mut emitter = single(renderer, None, spawn)["emitters"][0].clone();
        emitter["id"] = json!(id);
        emitter["blend"] = json!(blend);
        emitter
    };
    let mut sprite = document["emitters"][0].clone();
    sprite["id"] = json!("sprite");
    document["emitters"].as_array_mut().unwrap().extend([
        extra("square", "square", "alpha", "size = 2; alpha = 0.5;"),
        extra("glow", "square", "premultiplied", "size = 1; alpha = 0; r = 2;"),
        extra("spark", "streak", "additive", "size = 0.5; vx = 100;"),
        extra("faded", "soft", "alpha", "size = 1; alpha = 0;"),
        sprite,
    ]);
    let mut effect = runtime(&document);
    for (particle, z) in effect.particles_mut().iter_mut().zip([10.0, 0.0, -10.0, 5.0, 0.0, 20.0]) {
        particle.z = z;
    }
    let before = effect.particles().to_vec();
    let textures = prepared(&effect);

    let mut frame = ParticleFrame::new();
    frame.add_effect(&effect, Some(&textures)).unwrap();
    assert_eq!(effect.particles(), before.as_slice(), "drawing never mutates the simulation");

    // One batch per blend family and texture, in first-emitter order: both sprite emitters share a batch, straight and
    // premultiplied squares share the source-over batch, and the faded straight particle draws nothing while the
    // zero-alpha premultiplied one still adds light.
    let batches = frame.batches();
    assert_eq!(batches.len(), 3);
    assert_eq!((batches[0].texture(), batches[0].is_additive(), batches[0].instances()), (Some(0), false, 0..2));
    assert_eq!((batches[1].texture(), batches[1].is_additive(), batches[1].instances()), (None, false, 2..4));
    assert_eq!((batches[2].texture(), batches[2].is_additive(), batches[2].instances()), (None, true, 4..5));
    assert_eq!(frame.textures().len(), 1);
    assert!(frame.textures()[0].ptr_eq(textures.get(effect.definition().emitters()[0].texture_id.unwrap()).unwrap()));
    let depths: Vec<f32> = frame.instances().iter().map(|instance| instance.position().z).collect();
    assert_eq!(depths, [10.0, 20.0, 0.0, -10.0, 5.0], "instances keep generation order inside a batch");

    // Bounds hold what each batch draws, including the streak's trail behind its head.
    let streak = batches[2].bounds();
    assert!(streak.min.x <= 10.0 - (100.0 * 0.035) && streak.max.x >= 10.0, "{streak:?}");
    assert!(batches[0].bounds().min.z <= 8.0 && batches[0].bounds().max.z >= 22.0);

    // Transforms place effects in the world and scale sizes; clearing empties the frame for the next one.
    frame.clear();
    assert!(frame.is_empty() && frame.batches().is_empty() && frame.textures().is_empty());
    let placed = Mat4::from_scale_rotation_translation(Vec3::splat(2.0), Default::default(), Vec3::new(1.0, 0.0, 0.0));
    frame.add_transformed_effect(&effect, Some(&textures), &placed).unwrap();
    let first = frame.instances()[0];
    assert_eq!(first.position(), Vec3::new(21.0, 40.0, 20.0));
    assert_eq!(first.position_size[3], 4.0);
}

#[test]
fn textured_effects_need_their_prepared_images() {
    let effect = runtime(&single("textured", Some(texture(json!({}))), "size = 2;"));
    let mut frame = ParticleFrame::new();
    let colored = runtime(&single("soft", None, "size = 2;"));
    frame.add_effect(&colored, None).unwrap();
    assert_eq!(frame.add_effect(&effect, None), Err(ParticleRenderError::TexturedEffect));
    assert_eq!(frame.add_effect(&effect, Some(&ParticleTextureSet::new())), Err(ParticleRenderError::TextureNotReady));
    assert_eq!((frame.batches().len(), frame.instances().len(), frame.textures().len()), (1, 1, 0));
}

#[test]
fn textures_normalize_and_validate_sources() {
    let definition =
        compile_particle_effect(&single("flipbook", Some(texture(json!({"columns": 2, "rows": 2}))), "")).unwrap();
    let normalized = definition.asset().emitters[0].texture.clone().unwrap();
    assert_eq!(
        (normalized.columns, normalized.rows, normalized.frames, normalized.fps, normalized.looping),
        (2, 2, 4, 0.0, true)
    );
    assert_eq!(
        definition.emitters()[0].texture_dimensions,
        Some(ParticleTextureDimensions {
            width: 16,
            height: 16
        })
    );
    assert!(definition.asset().to_json()["emitters"][0].get("textureDimensions").is_none());
    assert_eq!(
        read_particle_texture_source(DEFAULT_PARTICLE_TEXTURE),
        Ok(ParticleTextureDimensions {
            width: 16,
            height: 16
        })
    );

    // Oversized headers and payloads are rejected before decoding.
    let png = base64_decode(DEFAULT_PARTICLE_TEXTURE.split(',').nth(1).unwrap());
    let mut wide = png.clone();
    wide[16..20].copy_from_slice(&2049_u32.to_be_bytes());
    let wide = format!("data:image/png;base64,{}", base64_encode(&wide));
    assert_eq!(read_particle_texture_source(&wide), Err(ParticleTextureError::InvalidDimensions));
    let huge = format!("data:image/png;base64,{}", base64_encode(&vec![0; 1024 * 1024 + 1]));
    assert_eq!(read_particle_texture_source(&huge), Err(ParticleTextureError::TooLarge));

    // Too many distinct sources.
    let mut many = single("textured", Some(texture(json!({}))), "");
    let emitter = many["emitters"][0].clone();
    many["emitters"] = Value::Array(
        (0..9)
            .map(|index| {
                let mut copy = emitter.clone();
                let mut bytes = png.clone();
                bytes.push(index as u8);
                copy["id"] = json!(format!("sprite{index}"));
                copy["texture"]["source"] = json!(format!("data:image/png;base64,{}", base64_encode(&bytes)));
                copy
            })
            .collect(),
    );
    let error = compile_particle_effect(&many).unwrap_err().to_string();
    assert!(error.contains("8 distinct"), "{error}");
}

#[test]
fn flipbook_frames_use_seconds_row_major_cells_and_lifetime() {
    let cases = [
        (json!({"fps": 4, "loop": false}), 1.5, [0.53125, 0.53125]),
        (json!({"fps": 4, "loop": true}), 1.5, [0.03125, 0.53125]),
        (json!({"fps": 0}), 1.25, [0.03125, 0.53125]),
    ];
    let cell = |effect: &ParticleEffect| {
        let mut frame = ParticleFrame::new();
        frame.add_effect(effect, Some(&prepared(effect))).unwrap();
        let [left, top, ..] = frame.instances()[0].uv_rect;
        [left, top]
    };
    let unorm = |corner: [f64; 2]| corner.map(|value| (value * UNORM16_MAXIMUM).round() as u16);
    for (options, age, expected) in cases {
        let mut options = options;
        options["columns"] = json!(2);
        options["rows"] = json!(2);
        let mut effect = runtime(&single("flipbook", Some(texture(options)), "size = 2; alpha = 0.5;"));
        effect.particles_mut()[0].age = age;
        assert_eq!(cell(&effect), unorm(expected), "age {age}");
    }
    let mut still =
        runtime(&single("textured", Some(texture(json!({"columns": 2, "rows": 2, "fps": 4}))), "size = 2;"));
    still.particles_mut()[0].age = 1.5;
    assert_eq!(cell(&still), unorm([0.03125, 0.03125]), "textured sprites show the first cell");
}

#[test]
fn texture_sets_stage_images_before_activation() {
    let textured = compile_particle_effect(&single("textured", Some(texture(json!({}))), "size = 2;")).unwrap();
    let colored = compile_particle_effect(&single("soft", None, "size = 2;")).unwrap();
    let effect = ParticleEffect::new(&textured, ParticleEffectOptions::default()).unwrap();
    let id = textured.emitters()[0].texture_id.unwrap();

    let mut set = ParticleTextureSet::new();
    assert!(set.get(id).is_none());
    set.prepare(&textured).unwrap();
    set.activate(Some(&textured));
    let image = set.get(id).unwrap().clone();
    assert!(set.get_source(DEFAULT_PARTICLE_TEXTURE).unwrap().ptr_eq(&image));
    assert_eq!((image.width(), image.height()), (16, 16));
    // The cross is white with transparent corners.
    assert_eq!(image.pixels()[3], 0);
    assert!(image.pixels().chunks(4).any(|pixel| pixel == [255, 255, 255, 255]));
    let mut frame = ParticleFrame::new();
    frame.add_effect(&effect, Some(&set)).unwrap();
    assert!(frame.textures()[0].ptr_eq(&image));

    // Preparing a replacement keeps the displayed image until the replacement is activated.
    set.prepare(&colored).unwrap();
    assert!(set.get(id).is_some());
    set.activate(Some(&colored));
    assert!(set.is_empty());
}

fn base64_decode(text: &str) -> Vec<u8> {
    let value = |byte: u8| match byte {
        b'A'..=b'Z' => byte - b'A',
        b'a'..=b'z' => byte - b'a' + 26,
        b'0'..=b'9' => byte - b'0' + 52,
        b'+' => 62,
        _ => 63,
    };
    let bytes: Vec<u8> = text.bytes().filter(|byte| *byte != b'=').collect();
    let mut output = Vec::new();
    for chunk in bytes.chunks(4) {
        let bits =
            chunk.iter().fold(0_u32, |bits, byte| (bits << 6) | u32::from(value(*byte))) << (6 * (4 - chunk.len()));
        output.extend_from_slice(&bits.to_be_bytes()[1..chunk.len()]);
    }
    output
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let bits = chunk.iter().fold(0_u32, |bits, byte| (bits << 8) | u32::from(*byte)) << (8 * (3 - chunk.len()));
        for index in 0..4 {
            if index <= chunk.len() {
                output.push(ALPHABET[((bits >> (18 - (6 * index))) & 63) as usize] as char);
            } else {
                output.push('=');
            }
        }
    }
    output
}
