// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Particle geometry and materials: camera-facing billboards, streak depth, textured and flipbook UVs, global depth
//! order across materials, read-only rendering, and texture-set staging. Ported from the TypeScript
//! `test/particle-materials.test.mjs` and `test/particle-3d.test.mjs`.

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
    PARTICLE_XY_BASIS,
    ParticleBatch,
    ParticleCameraBasis,
    ParticleRenderError,
    ParticleTextureSet,
    append_particle_effect,
    create_particle_effect_batches,
    create_particle_render_metrics,
};
use joy_renderer::{
    ColorVertex,
    OrbitCamera,
    TransparentDraw,
};
use serde_json::{
    Value,
    json,
};

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
    ParticleEffect::new(&definition, ParticleEffectOptions { position: [10.0, 20.0, 30.0], ..Default::default() })
        .unwrap()
}

fn texture(options: Value) -> Value {
    let mut texture = json!({"source": DEFAULT_PARTICLE_TEXTURE});
    if let (Value::Object(texture), Value::Object(options)) = (&mut texture, options) {
        texture.extend(options);
    }
    texture
}

fn textured_vertices(batch: &ParticleBatch) -> &[joy_renderer::TexturedVertex] {
    match batch {
        ParticleBatch::Textured { vertices, .. } => vertices,
        ParticleBatch::Colored(_) => panic!("expected a textured batch"),
    }
}

#[test]
fn billboards_follow_camera_axes_and_streaks_keep_view_depth() {
    let spawn = "size = 2; vx = 12; vy = 18; vz = 60; az = 6;";
    let soft = runtime(&single("soft", None, spawn));
    let before = soft.particles().to_vec();
    let side = OrbitCamera { yaw: std::f32::consts::FRAC_PI_2, pitch: 0.0, ..OrbitCamera::default() };
    let mut vertices = Vec::new();
    append_particle_effect(&mut vertices, &soft, &ParticleCameraBasis::from(side.basis())).unwrap();
    let xs: Vec<f32> = vertices.iter().map(|vertex| vertex.position[0]).collect();
    let zs: Vec<f32> = vertices.iter().map(|vertex| vertex.position[2]).collect();
    let spread = |values: &[f32]| {
        values.iter().cloned().fold(f32::MIN, f32::max) - values.iter().cloned().fold(f32::MAX, f32::min)
    };
    assert!(spread(&xs) < 1e-5, "side view billboard lies in YZ");
    assert!(spread(&zs) > 3.0);
    assert_eq!(soft.particles(), before.as_slice(), "rendering never mutates the simulation");

    let streak = runtime(&single("streak", None, spawn));
    let front = OrbitCamera { yaw: 0.0, pitch: 0.0, ..OrbitCamera::default() };
    let mut vertices = Vec::new();
    append_particle_effect(&mut vertices, &streak, &ParticleCameraBasis::from(front.basis())).unwrap();
    let nearest = vertices.iter().map(|vertex| vertex.position[2]).fold(f32::MAX, f32::min);
    assert!(nearest <= 30.0 - (60.0 * 0.035) + 1e-4, "the tail follows VZ even along the view axis");
    assert!(vertices.len().is_multiple_of(3) && vertices.iter().all(|vertex| vertex.color[0].is_finite()));
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
    assert_eq!(definition.emitters()[0].texture_dimensions, Some(ParticleTextureDimensions { width: 16, height: 16 }));
    assert!(definition.asset().to_json()["emitters"][0].get("textureDimensions").is_none());
    assert_eq!(
        read_particle_texture_source(DEFAULT_PARTICLE_TEXTURE),
        Ok(ParticleTextureDimensions { width: 16, height: 16 })
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
fn flipbook_frames_use_seconds_row_major_uvs_and_lifetime() {
    let cases = [
        (json!({"fps": 4, "loop": false}), 1.5, [0.53125, 0.53125]),
        (json!({"fps": 4, "loop": true}), 1.5, [0.03125, 0.53125]),
        (json!({"fps": 0}), 1.25, [0.03125, 0.53125]),
    ];
    for (options, age, expected) in cases {
        let mut options = options;
        options["columns"] = json!(2);
        options["rows"] = json!(2);
        let mut effect = runtime(&single("flipbook", Some(texture(options)), "size = 2; alpha = 0.5;"));
        effect.particles_mut()[0].age = age;
        let batches = create_particle_effect_batches(&effect, &PARTICLE_XY_BASIS, None, &[]).unwrap();
        let vertices = textured_vertices(&batches[0]);
        assert_eq!(vertices.len(), 6);
        assert_eq!(vertices[0].uv, expected.map(|value| value as f32));
        assert_eq!(batches[0].source(), Some(DEFAULT_PARTICLE_TEXTURE));
    }
    let mut still =
        runtime(&single("textured", Some(texture(json!({"columns": 2, "rows": 2, "fps": 4}))), "size = 2;"));
    still.particles_mut()[0].age = 1.5;
    let batches = create_particle_effect_batches(&still, &PARTICLE_XY_BASIS, None, &[]).unwrap();
    assert_eq!(textured_vertices(&batches[0])[0].uv, [0.03125, 0.03125]);
    assert_eq!(
        append_particle_effect(&mut Vec::new(), &still, &PARTICLE_XY_BASIS),
        Err(ParticleRenderError::TexturedEffect)
    );
}

#[test]
fn mixed_materials_keep_global_depth_order_and_merge_adjacent_runs() {
    let mut document = single("textured", Some(texture(json!({}))), "size = 2; alpha = 0.5;");
    let square = single("square", None, "size = 2; alpha = 0.5;")["emitters"][0].clone();
    let mut second = square.clone();
    second["id"] = json!("two");
    let mut third = document["emitters"][0].clone();
    third["id"] = json!("three");
    document["emitters"].as_array_mut().unwrap().extend([second, third]);
    let mut effect = runtime(&document);
    for (particle, z) in effect.particles_mut().iter_mut().zip([10.0, 0.0, -10.0]) {
        particle.z = z;
    }
    let before = effect.particles().to_vec();
    let first = create_particle_effect_batches(&effect, &PARTICLE_XY_BASIS, None, &[]).unwrap();
    let leading_z = |batches: &[ParticleBatch]| -> Vec<f32> {
        batches
            .iter()
            .map(|batch| match batch {
                ParticleBatch::Colored(vertices) => vertices[0].position[2],
                ParticleBatch::Textured { vertices, .. } => vertices[0].position[2],
            })
            .collect()
    };
    assert_eq!(leading_z(&first), [-10.0, 0.0, 10.0]);
    let sources: Vec<bool> = first.iter().map(|batch| batch.source().is_some()).collect();
    assert_eq!(sources, [true, false, true]);
    assert_eq!(create_particle_effect_batches(&effect, &PARTICLE_XY_BASIS, None, &[]).unwrap(), first);
    let reverse = ParticleCameraBasis { backward: [0.0, 0.0, -1.0], ..PARTICLE_XY_BASIS };
    assert_eq!(leading_z(&create_particle_effect_batches(&effect, &reverse, None, &[]).unwrap()), [10.0, 0.0, -10.0]);

    // Reference triangles interleave at their depth for either view direction.
    let corner = |x: f32, y: f32| ColorVertex { position: [x, y, 5.0], color: [0.2, 0.3, 0.4, 0.5] };
    let reference = [corner(-1.0, -1.0), corner(1.0, -1.0), corner(0.0, 1.0)];
    for backward in [[0.0, 0.0, 1.0], [0.0, 0.0, -1.0]] {
        let camera = ParticleCameraBasis { backward, ..PARTICLE_XY_BASIS };
        let mut metrics = create_particle_render_metrics(effect.definition());
        let mixed = create_particle_effect_batches(&effect, &camera, Some(&mut metrics), &reference).unwrap();
        let depths: Vec<f64> = mixed
            .iter()
            .flat_map(|batch| match batch {
                ParticleBatch::Colored(vertices) => {
                    vertices.chunks(3).map(|triangle| f64::from(triangle[0].position[2])).collect::<Vec<_>>()
                }
                ParticleBatch::Textured { vertices, .. } => {
                    vertices.chunks(3).map(|triangle| f64::from(triangle[0].position[2])).collect()
                }
            })
            .map(|z| z * backward[2])
            .collect();
        assert!(depths.windows(2).all(|pair| pair[0] <= pair[1]), "{depths:?}");
        assert!(depths.contains(&(5.0 * backward[2])));
        assert_eq!(metrics.emitters.iter().map(|row| row.triangles).sum::<u64>(), 6);
    }
    assert_eq!(effect.particles(), before.as_slice());
    assert_eq!(
        create_particle_effect_batches(&effect, &PARTICLE_XY_BASIS, None, &reference[..2]),
        Err(ParticleRenderError::IncompleteTriangles)
    );
    let mut broken = reference;
    broken[1].position[0] = f32::NAN;
    assert_eq!(
        create_particle_effect_batches(&effect, &PARTICLE_XY_BASIS, None, &broken),
        Err(ParticleRenderError::NonFiniteGeometry)
    );
    effect.particles_mut()[1].z = 20.0;
    assert_eq!(
        create_particle_effect_batches(&effect, &PARTICLE_XY_BASIS, None, &[]).unwrap().len(),
        2,
        "adjacent identical materials merge"
    );
}

#[test]
fn texture_sets_stage_images_before_activation() {
    let textured = compile_particle_effect(&single("textured", Some(texture(json!({}))), "size = 2;")).unwrap();
    let colored = compile_particle_effect(&single("soft", None, "size = 2;")).unwrap();
    let effect = ParticleEffect::new(&textured, ParticleEffectOptions::default()).unwrap();
    let batches = create_particle_effect_batches(&effect, &PARTICLE_XY_BASIS, None, &[]).unwrap();

    let mut set = ParticleTextureSet::new();
    assert_eq!(set.transparent_draws(&batches).unwrap_err(), ParticleTextureError::NotReady);
    set.prepare(&textured).unwrap();
    set.activate(Some(&textured));
    let image = set.get(DEFAULT_PARTICLE_TEXTURE).unwrap().clone();
    assert_eq!((image.width(), image.height()), (16, 16));
    // The cross is white with transparent corners.
    assert_eq!(image.pixels()[3], 0);
    assert!(image.pixels().chunks(4).any(|pixel| pixel == [255, 255, 255, 255]));
    let draws = set.transparent_draws(&batches).unwrap();
    assert!(
        matches!(draws[0], TransparentDraw::Textured { texture, vertices } if texture.ptr_eq(&image) && vertices.len() == 6)
    );

    // Preparing a replacement keeps the displayed image until the replacement is activated.
    set.prepare(&colored).unwrap();
    assert!(set.get(DEFAULT_PARTICLE_TEXTURE).is_ok());
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
