// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Environment frames checked against `tests/fixtures/environment-reference.json`, a golden recording of the frames a
//! range of latitudes, times of day, clock rates, and winds produce. Re-record with `JOY_UPDATE_FIXTURES=1` when the
//! environment changes on purpose.

use joy_environment::{
    AtmosphereLight,
    CelestialBody,
    EnvironmentFrame,
    EnvironmentOptions,
    EnvironmentSystem,
};
use joy_test_support::{
    Tolerance,
    golden_file,
    json_f32,
    json_f32s,
};
use serde_json::{
    Value,
    json,
};

/// Agreement required between recorded and produced values: frames compute in `f64` and store `f32`.
const TOLERANCE: f64 = 1e-5;

/// One recorded celestial light.
fn light_record(light: &AtmosphereLight) -> Value {
    let kind = match light.kind {
        CelestialBody::Sun => "sun",
        CelestialBody::Moon => "moon",
    };
    let light = &light.light;
    json!({
        "kind": kind,
        "direction": json_f32s(&light.direction.to_array()),
        "color": json_f32s(&light.color.rgb_array()),
        "intensity": json_f32(light.intensity),
        "angularRadius": json_f32(light.angular_radius),
        "castsShadow": light.casts_shadow,
    })
}

/// One recorded frame.
fn frame_record(frame: &EnvironmentFrame) -> Value {
    json!({
        "elapsedSeconds": frame.elapsed_seconds,
        "timeOfDay": frame.time_of_day,
        "sun": light_record(&frame.sun),
        "moon": light_record(&frame.moon),
        "primary": if frame.primary_light == frame.sun { "sun" } else { "moon" },
        "skylight": {
            "color": json_f32s(&frame.skylight.color.rgb_array()),
            "groundColor": json_f32s(&frame.skylight.ground_color.rgb_array()),
            "intensity": json_f32(frame.skylight.intensity),
        },
        "wind": {
            "direction": json_f32s(&frame.wind.direction.to_array()),
            "velocity": json_f32s(&frame.wind.velocity.to_array()),
            "speed": json_f32(frame.wind.speed),
        },
    })
}

#[test]
fn frames_match_the_recording() {
    let golden = golden_file!("environment-reference.json");
    let mut produced = golden.recording().clone();
    let cases = produced["cases"].as_array_mut().unwrap();
    assert!(cases.len() >= 6);

    // Each case replays its options (authored records, in the document spellings) and clock steps.
    for case in cases {
        let options = EnvironmentOptions::from_json(&case["options"]).unwrap();
        let mut environment = EnvironmentSystem::new(options).unwrap();
        let mut frame = environment.sample();
        for delta in case["steps"].as_array().unwrap() {
            frame = environment.step(delta.as_f64().unwrap()).unwrap();
        }
        case["frame"] = frame_record(&frame);
    }
    golden.assert_matches(&produced, &Tolerance::relative(TOLERANCE));
}

#[test]
fn clock_is_deterministic_and_wraps_into_the_next_day() {
    let options =
        EnvironmentOptions { time_of_day: 0.25, day_length_seconds: 100.0, time_scale: 2.0, ..Default::default() };
    let mut first = EnvironmentSystem::new(options).unwrap();
    let mut second = EnvironmentSystem::new(options).unwrap();
    let frame = first.step(10.0).unwrap();
    assert!((frame.time_of_day - 0.45).abs() < f64::EPSILON);
    assert_eq!(frame, second.step(10.0).unwrap());
    assert_eq!(frame.primary_light.kind, CelestialBody::Sun);

    let options = EnvironmentOptions { time_of_day: 0.95, day_length_seconds: 10.0, ..Default::default() };
    let mut wrapping = EnvironmentSystem::new(options).unwrap();
    assert_eq!(wrapping.step(1.0).unwrap().time_of_day, 0.050000000000000044);
    assert_eq!(wrapping.sample().primary_light.kind, CelestialBody::Moon);
}
