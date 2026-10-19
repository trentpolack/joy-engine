// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/config_session.rs`.

use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use joy_core::ConfigApplyMode;

pub(crate) fn fields() -> Vec<ConfigField> {
    let field = |key: &str, group: &str, default_value, max, step, apply_mode| ConfigField {
        key: key.to_owned(),
        label: key.to_owned(),
        group: group.to_owned(),
        description: format!("{key} description."),
        default_value,
        min: if key == "speed" { 1.0 } else { 0.0 },
        max,
        step,
        units: String::new(),
        apply_mode,
    };
    vec![
        field("speed", "Movement", 4.0, 10.0, 0.5, ConfigApplyMode::Live),
        field("count", "Spawning", 3.0, 8.0, 1.0, ConfigApplyMode::NextSpawn),
    ]
}

fn values(pairs: &[(&str, f64)]) -> IndexMap<String, f64> {
    pairs.iter().map(|(key, value)| ((*key).to_owned(), *value)).collect()
}

#[test]
fn invalid_edits_are_atomic() {
    let mut session = ConfigSession::new(&fields(), &values(&[("speed", 5.0)])).unwrap();
    session.set_value("speed", 7.0).unwrap();
    assert_eq!(session.values(), &values(&[("speed", 7.0), ("count", 3.0)]));
    assert_eq!(session.take_change(), Some(values(&[("speed", 7.0), ("count", 3.0)])));
    assert_eq!(session.take_change(), None);
    assert!(matches!(session.set_value("count", 2.5), Err(ConfigSessionError::Config(ConfigError::NotAnInteger(_)))));
    assert!(session.set_value("unknown", 2.0).is_err());
    assert_eq!(session.values(), &values(&[("speed", 7.0), ("count", 3.0)]));
}

#[test]
fn undo_revert_and_resets_preserve_saved_and_default_semantics() {
    let mut session = ConfigSession::new(&fields(), &values(&[("speed", 5.0), ("count", 4.0)])).unwrap();
    session.set_value("speed", 6.0).unwrap();
    session.set_value("count", 7.0).unwrap();
    assert!(session.can_undo());
    session.undo();
    assert_eq!(session.values(), &values(&[("speed", 6.0), ("count", 4.0)]));
    session.reset_field("speed").unwrap();
    assert_eq!(session.value("speed"), Some(4.0));
    session.undo();
    assert_eq!(session.value("speed"), Some(6.0));
    session.reset_all().unwrap();
    assert_eq!(session.values(), &values(&[("speed", 4.0), ("count", 3.0)]));
    session.revert().unwrap();
    assert_eq!(session.values(), &values(&[("speed", 5.0), ("count", 4.0)]));
    assert!(!session.dirty());
}

#[test]
fn continuous_edits_undo_as_one_step() {
    let mut session = ConfigSession::new(&fields(), &IndexMap::new()).unwrap();
    session.begin_edit();
    for speed in [4.5, 5.0, 5.5] {
        session.set_value("speed", speed).unwrap();
    }
    session.end_edit();
    session.set_value("speed", 6.0).unwrap();
    session.undo();
    assert_eq!(session.value("speed"), Some(5.5));
    session.undo();
    assert_eq!(session.value("speed"), Some(4.0));
    assert!(!session.can_undo());
    session.redo();
    assert_eq!(session.value("speed"), Some(5.5));
}

#[test]
fn saves_snapshot_the_current_values_and_later_edits_stay_dirty() {
    let saved = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&saved);
    let mut session =
        ConfigSession::new(&fields(), &values(&[("speed", 5.0)])).unwrap().with_save(Box::new(move |values| {
            sink.borrow_mut().push(values.clone());
            Ok(())
        }));
    session.set_value("speed", 6.0).unwrap();
    session.save().unwrap();
    session.set_value("speed", 7.0).unwrap();
    assert_eq!(*saved.borrow(), [values(&[("speed", 6.0), ("count", 3.0)])]);
    assert!(session.dirty());
    session.revert().unwrap();
    assert_eq!(session.value("speed"), Some(6.0));

    let mut unsaved = ConfigSession::new(&fields(), &IndexMap::new()).unwrap();
    assert!(!unsaved.can_save());
    assert_eq!(unsaved.save(), Err(ConfigSessionError::NoSaveAdapter));
    let mut failing =
        ConfigSession::new(&fields(), &IndexMap::new()).unwrap().with_save(Box::new(|_| Err("disk full".to_owned())));
    failing.set_value("speed", 6.0).unwrap();
    assert_eq!(failing.save(), Err(ConfigSessionError::Save("disk full".to_owned())));
    assert!(failing.dirty());
}

#[test]
fn runtime_rejection_leaves_values_and_history_unchanged() {
    let mut session =
        ConfigSession::new(&fields(), &values(&[("speed", 5.0)])).unwrap().with_on_change(Box::new(|values| {
            if values["speed"] == 7.0 { Err("runtime rejected value".to_owned()) } else { Ok(()) }
        }));
    assert_eq!(session.set_value("speed", 7.0), Err(ConfigSessionError::Rejected("runtime rejected value".into())));
    assert_eq!(session.value("speed"), Some(5.0));
    assert!(!session.can_undo());
    session.set_value("speed", 6.0).unwrap();
    session.undo();
    assert_eq!(session.value("speed"), Some(5.0));
    assert!(!session.can_undo());
}
