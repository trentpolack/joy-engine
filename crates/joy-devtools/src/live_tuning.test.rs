// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/live_tuning.rs`.

use super::*;

fn field(key: &str, value: f64) -> TuningField {
    TuningField {
        key: key.to_owned(),
        label: key.to_owned(),
        description: format!("{key} description."),
        value,
        default_value: None,
        min: 0.0,
        max: 100.0,
        step: 0.5,
        units: String::new(),
        group: None,
    }
}

#[test]
fn registered_objects_share_one_history_keyed_by_identifier() {
    let mut tuning = LiveTuning::new("PLAYGROUND TUNING");
    tuning.register("kart1", &[field("stiffness", 30.0), field("damping", 4.0)]).unwrap();
    tuning.register("camera", &[field("distance", 12.0)]).unwrap();
    assert_eq!(tuning.value("kart1", "damping"), Some(4.0));

    tuning.session().set_value("kart1.stiffness", 35.0).unwrap();
    tuning.session().set_value("camera.distance", 14.0).unwrap();
    let change = tuning.take_change().unwrap();
    assert_eq!((change["kart1.stiffness"], change["camera.distance"]), (35.0, 14.0));
    tuning.session().undo();
    assert_eq!(tuning.value("camera", "distance"), Some(12.0));
    // Resetting returns to the registered value.
    tuning.session().reset_field("kart1.stiffness").unwrap();
    assert_eq!(tuning.value("kart1", "stiffness"), Some(30.0));
    assert_eq!(tuning.session().fields()[2].group, "camera");

    let preset = tuning.capture_preset();
    tuning.session().set_value("kart1.damping", 9.0).unwrap();
    tuning.apply_preset(&preset).unwrap();
    assert_eq!(tuning.value("kart1", "damping"), Some(4.0));
}

#[test]
fn registration_is_validated_and_closes_once_the_session_opens() {
    let mut tuning = LiveTuning::new("T");
    assert_eq!(tuning.register("1kart", &[field("a", 1.0)]), Err(TuningError::InvalidIdentifier));
    assert_eq!(tuning.register("kart.one", &[field("a", 1.0)]), Err(TuningError::InvalidIdentifier));
    assert!(matches!(tuning.register("kart", &[field("a", 500.0)]), Err(TuningError::Config(_))));
    tuning.register("kart", &[field("a", 1.0)]).unwrap();
    assert_eq!(tuning.register("kart", &[field("b", 1.0)]), Err(TuningError::DuplicateIdentifier("kart".into())));
    tuning.session();
    assert_eq!(tuning.register("camera", &[field("b", 1.0)]), Err(TuningError::SessionOpen));
}
