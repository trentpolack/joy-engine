// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use joy_core::SeededRandom;

use super::*;

#[test]
fn cues_render_identically_from_the_same_seed_and_stay_in_range() {
    let synth = Synth::new(22_050);
    let render = || {
        let mut random = SeededRandom::new(7);
        let layers = vec![
            synth.noise_burst(&mut random, 0.3, 9.0, 0.4, 1.2),
            synth.bent_tone(90.0, 0.3, 6.0, 1.0),
            synth.crackle(&mut random, 0.3, 0.8),
        ];
        synth.mix_normalized(0.3, &layers, 0.95, 0.004)
    };
    let first = render();
    assert_eq!(first, render(), "seeded layers replay exactly");
    assert!(first.iter().all(|sample| sample.abs() <= 0.95 + 1e-6), "the mix is normalized");
    assert_eq!(*first.last().unwrap(), 0.0, "the tail fades out against clicks");

    let mut voices = VoiceMix::new(44_100);
    voices.tone(Waveform::Square, 880.0, 440.0, 0.0, 0.1, 0.4, 0.0);
    voices.burst(0.05, 0.1, 0.3, FilterKind::Bandpass, 800.0, 2400.0);
    let samples = voices.finish();
    assert!(samples.len() >= ((0.15 * 44_100.0) as usize));
    assert!(samples.iter().all(|sample| sample.abs() <= 1.0));
}

#[test]
fn envelopes_ramp_up_quickly_and_release_to_the_floor() {
    assert!((percussive_envelope(0.0, 0.5, 0.1) - ENVELOPE_FLOOR).abs() < 1e-12);
    assert!((percussive_envelope(0.012, 0.5, 0.1) - 0.1).abs() < 1e-9);
    assert!(percussive_envelope(0.25, 0.5, 0.1) < 0.1);
    assert!((percussive_envelope(0.6, 0.5, 0.1) - ENVELOPE_FLOOR).abs() < 1e-12);
}
