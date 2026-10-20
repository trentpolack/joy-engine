// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/native.rs`.

use super::*;

#[test]
fn mixing_resamples_sums_clamps_and_retires_finished_voices() {
    let mut mixer = Mixer {
        sounds: vec![
            DecodedSound {
                sample_rate: 24_000,
                frames: vec![[0.0, 0.0], [1.0, -1.0]],
            },
            DecodedSound {
                sample_rate: 48_000,
                frames: vec![[0.75, 0.75]; 3],
            },
        ],
        voices: vec![
            MixVoice {
                id: 0,
                sound: 0,
                position: 0.0,
                gain: 1.0,
            },
            MixVoice {
                id: 1,
                sound: 1,
                position: 0.0,
                gain: 0.5,
            },
        ],
        output_rate: 48_000.0,
        next_id: 2,
    };
    let mut frames = vec![[0.0; 2]; 2];
    mixer.mix(&mut frames);
    // Half-rate sound: frame 0, then halfway to frame 1; plus 0.375 from the second voice.
    assert_eq!(frames, [[0.375, 0.375], [0.875, -0.125]]);
    assert_eq!(mixer.voices.len(), 2);

    let mut frames = vec![[0.0; 2]; 2];
    mixer.mix(&mut frames);
    // The first voice holds its last frame through its final half step, then ends; the second plays its third
    // frame and ends.
    assert_eq!(frames, [[1.0, -0.625], [1.0, -1.0]]);
    assert!(mixer.voices.is_empty());
}
