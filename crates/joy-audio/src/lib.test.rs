// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/lib.rs`.

use super::*;

fn voices(sounds: &[usize]) -> Vec<VoiceRecord> {
    sounds
        .iter()
        .enumerate()
        .map(|(id, sound)| VoiceRecord {
            id: id as u64,
            sound: *sound,
        })
        .collect()
}

#[test]
fn stealing_retires_the_oldest_matching_voice_then_the_oldest_voice() {
    // Under both limits nothing is stolen.
    assert!(voices_to_steal(&voices(&[0, 1]), 0, 2, 4).is_empty());
    // At the sound's limit, its oldest voice goes.
    assert_eq!(voices_to_steal(&voices(&[1, 0, 0]), 0, 2, 8), [1]);
    // At the bank's limit, the oldest voice of any sound goes.
    assert_eq!(voices_to_steal(&voices(&[1, 0, 2]), 3, 4, 3), [0]);
    // Both: the matching voice frees one slot, so the bank limit is checked after it.
    assert_eq!(voices_to_steal(&voices(&[1, 0, 0]), 0, 2, 3), [1]);
    assert_eq!(voices_to_steal(&voices(&[1, 0, 0, 2]), 0, 2, 3), [1, 0]);
}

#[test]
fn invalid_definitions_are_reported_before_any_output_opens() {
    let silence = || {
        SoundDefinition::pcm(PcmSound {
            sample_rate: 8_000,
            channels: 1,
            samples: vec![0.0; 8],
        })
    };
    let error = AudioBank::new(
        [("blip", silence())],
        AudioBankOptions {
            maximum_voices: 0,
        },
    )
    .unwrap_err();
    assert_eq!(error, AudioError::InvalidVoiceLimit);
    let error = AudioBank::new([("blip", silence().with_maximum_voices(0))], AudioBankOptions::default());
    assert_eq!(error.unwrap_err(), AudioError::InvalidVoiceLimit);
    let error = AudioBank::new([("blip", silence().with_volume(1.5))], AudioBankOptions::default());
    assert_eq!(error.unwrap_err(), AudioError::InvalidVolume("blip".to_owned()));
    let broken = SoundDefinition::wav(&b"not a wav"[..]);
    let error = AudioBank::new([("broken", broken)], AudioBankOptions::default()).unwrap_err();
    assert!(matches!(error, AudioError::Load { ref name, .. } if name == "broken"), "{error}");
}

#[test]
fn master_volume_clamps_and_ignores_non_finite_values() {
    let tone = PcmSound {
        sample_rate: 22_050,
        channels: 1,
        samples: vec![0.25; 2_205],
    };
    let mut bank = AudioBank::new([("tone", SoundDefinition::pcm(tone))], AudioBankOptions::default()).unwrap();
    assert_eq!(bank.master_volume(), 1.0);
    bank.set_master_volume(0.4);
    assert_eq!(bank.master_volume(), 0.4);
    bank.set_master_volume(f32::NAN);
    assert_eq!(bank.master_volume(), 0.4);
    bank.set_master_volume(3.0);
    assert_eq!(bank.master_volume(), 1.0);
    bank.set_master_volume(-1.0);
    assert_eq!(bank.master_volume(), 0.0);
}

#[test]
fn unknown_or_destroyed_sounds_do_not_play() {
    let tone = PcmSound {
        sample_rate: 22_050,
        channels: 1,
        samples: vec![0.25; 2_205],
    };
    let mut bank = AudioBank::new([("tone", SoundDefinition::pcm(tone))], AudioBankOptions::default()).unwrap();
    assert!(bank.contains("tone"));
    assert!(!bank.play("missing"));
    bank.destroy();
    bank.destroy();
    assert!(!bank.play("tone") && !bank.resume() && !bank.running());
    assert_eq!(bank.active_voice_count(), 0);
}
