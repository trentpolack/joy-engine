// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Environment parity with frames recorded from the TypeScript `EnvironmentSystem`.

use joy_engine::environment::{AtmosphereLight, CelestialBody, EnvironmentOptions, EnvironmentSystem};
use serde_json::Value;

/// Frames recorded from the TypeScript engine; see the fixture's `source` field.
const REFERENCE: &str = include_str!("../fixtures/environment-reference.json");

/// Agreement required between the f64 TypeScript values and the stored f32 values.
const TOLERANCE: f64 = 1e-5;

fn assert_close(label: &str, field: &str, actual: f64, expected: f64) {
    assert!((actual - expected).abs() <= TOLERANCE, "{label} {field}: {actual} vs {expected}");
}

fn assert_triple(label: &str, field: &str, actual: [f32; 3], expected: &Value) {
    for (axis, value) in actual.iter().enumerate() {
        assert_close(label, &format!("{field}[{axis}]"), f64::from(*value), expected[axis].as_f64().unwrap());
    }
}

fn assert_light(label: &str, light: &AtmosphereLight, expected: &Value) {
    let kind = match light.kind {
        CelestialBody::Sun => "sun",
        CelestialBody::Moon => "moon",
    };
    assert_eq!(kind, expected["kind"], "{label} kind");
    assert_triple(label, &format!("{kind}.direction"), light.direction.to_array(), &expected["direction"]);
    assert_triple(label, &format!("{kind}.color"), light.color, &expected["color"]);
    assert_close(
        label,
        &format!("{kind}.intensity"),
        f64::from(light.intensity),
        expected["intensity"].as_f64().unwrap(),
    );
    let radius = expected["angularRadius"].as_f64().unwrap();
    assert_close(label, &format!("{kind}.angularRadius"), f64::from(light.angular_radius), radius);
    assert!(light.casts_shadow);
}

#[test]
fn frames_match_typescript_reference() {
    let reference: Value = serde_json::from_str(REFERENCE).unwrap();
    let cases = reference["cases"].as_array().unwrap();
    assert!(cases.len() >= 6);

    for case in cases {
        let label = case["label"].as_str().unwrap();
        // Authored option records use the TypeScript spellings, so they deserialize directly.
        let options = EnvironmentOptions::from_json(&case["options"]).unwrap();
        let mut environment = EnvironmentSystem::new(options).unwrap();
        let mut frame = environment.sample();
        for delta in case["steps"].as_array().unwrap() {
            frame = environment.step(delta.as_f64().unwrap()).unwrap();
        }

        let expected = &case["frame"];
        assert_close(label, "elapsedSeconds", frame.elapsed_seconds, expected["elapsedSeconds"].as_f64().unwrap());
        assert_close(label, "timeOfDay", frame.time_of_day, expected["timeOfDay"].as_f64().unwrap());
        assert_light(label, &frame.sun, &expected["sun"]);
        assert_light(label, &frame.moon, &expected["moon"]);
        assert_eq!(frame.primary_light, if expected["primary"] == "sun" { frame.sun } else { frame.moon }, "{label}");
        assert_triple(label, "skylight.color", frame.skylight.color, &expected["skylight"]["color"]);
        let skylight = expected["skylight"]["intensity"].as_f64().unwrap();
        assert_close(label, "skylight.intensity", f64::from(frame.skylight.intensity), skylight);
        assert_triple(label, "wind.direction", frame.wind.direction.to_array(), &expected["wind"]["direction"]);
        assert_triple(label, "wind.velocity", frame.wind.velocity.to_array(), &expected["wind"]["velocity"]);
        assert_close(label, "wind.speed", f64::from(frame.wind.speed), expected["wind"]["speed"].as_f64().unwrap());
        assert_eq!(frame.clouds, options.clouds);
    }
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
