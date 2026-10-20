// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/environment_system.rs`.

use super::*;

#[test]
fn partial_authored_records_keep_defaults_and_reject_unknown_fields() {
    let options = EnvironmentOptions::from_json(&serde_json::json!({"timeOfDay": 0.5, "wind": {"speed": 9}})).unwrap();
    assert_eq!(options.time_of_day, 0.5);
    assert_eq!(options.wind.speed, 9.0);
    assert_eq!(options.wind.gust_period_seconds, 8.0);
    assert_eq!(options.wind.gust_strength, 0.25);

    let unknown = EnvironmentOptions::from_json(&serde_json::json!({"weather": "rain"})).unwrap_err();
    assert!(unknown.to_string().contains("weather"), "{unknown}");
    let nested = EnvironmentOptions::from_json(&serde_json::json!({"wind": {"gustStrength": 2}})).unwrap_err();
    assert!(nested.to_string().contains("wind.gustStrength"), "{nested}");
}

#[test]
fn invalid_policy_and_steps_leave_state_unchanged() {
    let options = EnvironmentOptions {
        time_of_day: 1.0,
        ..Default::default()
    };
    assert_eq!(EnvironmentSystem::new(options).unwrap_err(), EnvironmentError::TimeOfDayNotBelowOne);
    let gusts = EnvironmentOptions {
        wind: WindOptions {
            gust_period_seconds: 0.0,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(EnvironmentSystem::new(gusts).unwrap_err().to_string().contains("gustPeriodSeconds"));

    let mut environment = EnvironmentSystem::new(EnvironmentOptions::default()).unwrap();
    let before = environment.clone();
    assert!(environment.step(-1.0).unwrap_err().to_string().contains("deltaSeconds"));
    assert!(environment.step(f64::NAN).is_err());
    assert_eq!(environment.set_time_of_day(1.0), Err(EnvironmentError::TimeOfDayNotBelowOne));
    environment.set_elapsed_seconds(f64::INFINITY);
    assert_eq!(environment, before);
}

#[test]
fn paused_day_still_advances_wind_time() {
    let options = EnvironmentOptions {
        time_of_day: 0.5,
        day_length_seconds: 0.0,
        ..Default::default()
    };
    let mut environment = EnvironmentSystem::new(options).unwrap();
    let frame = environment.step(60.0).unwrap();
    assert_eq!(frame.time_of_day, 0.5);
    assert_eq!(frame.elapsed_seconds, 60.0);
}
