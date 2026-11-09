// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use super::*;

fn settings(decay: ShakeDecay) -> CameraShakeSettings {
    CameraShakeSettings {
        decay,
        response: ShakeResponse::Squared,
        maximum_level: 1.0,
        rest_level: 0.0,
        axes: [ShakeAxis::wave(1.0, ShakeWave::sine(10.0)), ShakeAxis::STILL, ShakeAxis::STILL],
    }
}

#[test]
fn trauma_saturates_decays_to_rest_and_replays_identically() {
    let mut shake = CameraShake::new(settings(ShakeDecay::LinearPerSecond(2.0)));
    shake.add(0.7);
    shake.add(0.7);
    assert_eq!(shake.level(), 1.0, "trauma saturates");

    // The same sequence of hits and frames produces the same offsets.
    let mut replay = shake.clone();
    for _ in 0..10 {
        shake.advance(0.05);
        replay.advance(0.05);
        assert_eq!(shake.offset(), replay.offset());
    }
    assert!(shake.level() < 1e-12, "a second of linear decay empties full trauma");
    shake.advance(0.05);
    assert_eq!(shake.level(), 0.0);
    assert_eq!(shake.offset(), DVec3::ZERO);

    // Raising keeps the strongest hit; per-step retention ignores frame duration.
    let mut ring = CameraShake::new(settings(ShakeDecay::RetainedPerStep(0.5)));
    ring.raise_to(4.0);
    ring.raise_to(2.0);
    ring.advance(10.0);
    assert_eq!(ring.level(), 2.0);
}
