// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The TypeScript `test/texture-lifecycle.test.js`, adapted to synchronous decoding: renders before an effect's images
//! are ready never draw textured particles; a failed decode keeps the active images; and replacing or releasing a
//! definition drops images it no longer uses. The browser's asynchronous supersede and dispose races have no
//! counterpart, because Rust decodes during prepare.

use joy_engine::particles::{
    DEFAULT_PARTICLE_TEXTURE,
    ParticleEffect,
    ParticleEffectOptions,
    compile_particle_effect,
    encode_particle_texture_source,
};
use joy_engine::rendering::FrameInputs;
use joy_engine::runtime::{
    Frame,
    Game,
    GameResult,
    RenderTarget,
};
use joy_engine::serde_json::{
    Value,
    json,
};
use joy_engine::{
    HeadlessOptions,
    HeadlessShell,
};
use particle_lab::viewport::{
    ParticleViewport,
    PreviewRegion,
};

/// Distinct embedded PNG sources: the default texture with one trailing byte each (trailing bytes do not change the
/// image header or pixels).
fn sources() -> Vec<String> {
    let payload = DEFAULT_PARTICLE_TEXTURE.strip_prefix("data:image/png;base64,").unwrap();
    let png = decode_base64(payload);
    (0..3u8)
        .map(|index| {
            let mut bytes = png.clone();
            bytes.push(index);
            encode_particle_texture_source(&bytes).unwrap()
        })
        .collect()
}

/// A source whose header is valid but whose pixel data is corrupt.
fn corrupt_source() -> String {
    let payload = DEFAULT_PARTICLE_TEXTURE.strip_prefix("data:image/png;base64,").unwrap();
    let mut png = decode_base64(payload);
    for byte in png.iter_mut().skip(41).take(8) {
        *byte ^= 0x5a;
    }
    encode_particle_texture_source(&png).unwrap()
}

fn decode_base64(text: &str) -> Vec<u8> {
    let value = |byte: u8| -> u32 {
        match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'+' => 62,
            _ => 63,
        }
    };
    let mut output = Vec::new();
    for chunk in text.as_bytes().chunks(4) {
        let padding = chunk.iter().filter(|byte| **byte == b'=').count();
        let bits = chunk.iter().filter(|byte| **byte != b'=').fold(0u32, |bits, byte| (bits << 6) | value(*byte))
            << (6 * padding);
        output.extend_from_slice(&[(bits >> 16) as u8, (bits >> 8) as u8, bits as u8][..3 - padding]);
    }
    output
}

fn definition(sources: &[&str]) -> joy_engine::particles::CompiledParticleEffect {
    let emitters: Vec<Value> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            json!({"id": format!("sprite{index}"), "enabled": true, "root": true, "duration": 0, "rate": 0,
                "bursts": [{"time": 0, "count": 1}], "lifetime": 2, "renderer": "textured",
                "texture": {"source": source, "columns": 1, "rows": 1, "frames": 1, "fps": 12, "loop": true},
                "spawn": "size = 1;", "update": ""})
        })
        .collect();
    let emitters = if emitters.is_empty() {
        vec![
            json!({"id": "plain", "enabled": true, "root": true, "duration": 0, "rate": 0, "bursts": [], "lifetime": 1,
            "renderer": "soft", "spawn": "", "update": ""}),
        ]
    } else {
        emitters
    };
    compile_particle_effect(
        &json!({"version": 1, "name": "Textures", "maxParticles": 16, "parameters": {}, "emitters": emitters}),
    )
    .unwrap()
}

/// Draws one effect through a viewport whose images may not be ready yet.
struct TexturedPreview {
    viewport: ParticleViewport,
    effect: ParticleEffect,
    result: Option<Result<(), String>>,
}

impl Game for TexturedPreview {
    fn render(&mut self, frame: &mut Frame<'_>, target: &mut RenderTarget<'_>) -> GameResult {
        let region = PreviewRegion::full(frame.viewport.logical_size);
        self.result = Some(self.viewport.render(target, Some(&self.effect), region));
        if self.result.as_ref().is_some_and(Result::is_err) {
            target.draw(
                &joy_engine::rendering::RenderScene::new(),
                &joy_engine::rendering::OrbitCamera::default(),
                &FrameInputs::default(),
            )?;
        }
        Ok(())
    }
}

#[test]
fn renders_before_images_are_ready_stay_safe_and_draw_textures_once_committed() {
    let options = HeadlessOptions { width: 64, height: 64, ..Default::default() };
    let Ok(mut shell) = HeadlessShell::new(options) else {
        eprintln!("skipping GPU test: no adapter");
        return;
    };
    let sources = sources();
    let definition = definition(&[&sources[0]]);
    let effect = ParticleEffect::new(&definition, ParticleEffectOptions::default()).unwrap();
    let mut preview = TexturedPreview { viewport: ParticleViewport::new(), effect, result: None };
    preview.viewport.grid_visible = false;

    // A resize-time render before the images are committed draws no textured batch and reports no error.
    shell.step(&mut preview).unwrap();
    assert_eq!(preview.result.take(), Some(Ok(())));
    assert!(!preview.viewport.textures_ready());
    assert_eq!(preview.viewport.stats().scene_batches, 0);

    preview.viewport.prepare_effect(&definition).unwrap();
    preview.viewport.set_effect(Some(&definition));
    shell.step(&mut preview).unwrap();
    assert_eq!(preview.result.take(), Some(Ok(())));
    assert_eq!(preview.viewport.stats().scene_batches, 1, "the textured sprite draws");
    assert_eq!(preview.viewport.textures().len(), 1);
}

#[test]
fn failed_decoding_preserves_active_textures() {
    let sources = sources();
    let mut viewport = ParticleViewport::new();
    let active = definition(&[&sources[0]]);
    viewport.prepare_effect(&active).unwrap();
    viewport.set_effect(Some(&active));
    let retained = viewport.textures().get(&sources[0]).unwrap().clone();

    let corrupt = corrupt_source();
    let failing = definition(&[&sources[1], &corrupt]);
    assert!(viewport.prepare_effect(&failing).is_err(), "corrupt pixels fail during prepare");
    assert!(viewport.textures().get(&sources[0]).unwrap().ptr_eq(&retained), "the active image is untouched");
    assert_eq!(viewport.textures().len(), 1, "nothing from the failed prepare is kept");
    assert!(viewport.textures_ready());
}

#[test]
fn replacing_or_releasing_a_definition_drops_obsolete_images() {
    let sources = sources();
    let mut viewport = ParticleViewport::new();
    let first = definition(&[&sources[0], &sources[1]]);
    viewport.prepare_effect(&first).unwrap();
    viewport.set_effect(Some(&first));
    assert_eq!(viewport.textures().len(), 2);

    // Superseding: a new definition's prepare keeps the displayed images until it is committed.
    let second = definition(&[&sources[2]]);
    viewport.prepare_effect(&second).unwrap();
    assert_eq!(viewport.textures().len(), 3);
    viewport.set_effect(Some(&second));
    assert_eq!(viewport.textures().len(), 1);
    assert!(viewport.textures().get(&sources[0]).is_err());

    // Untextured effects and disposal release everything.
    let plain = definition(&[]);
    viewport.prepare_effect(&plain).unwrap();
    viewport.set_effect(Some(&plain));
    assert!(viewport.textures().is_empty());
    viewport.prepare_effect(&second).unwrap();
    viewport.set_effect(Some(&second));
    viewport.release_textures();
    assert!(viewport.textures().is_empty() && !viewport.textures_ready());
}
