// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/volume_file.rs`.

use std::fs;
use std::path::Path;

use super::*;
use serde_json::json;

fn document(path: &Path, exposure: f64) {
    let text = serde_json::to_string_pretty(&json!({
        "version": 1,
        "name": "Test Level",
        "entities": [
            {"id": "crate", "components": {}},
            {"id": "look", "components": {"postProcess": {"unbounded": true, "exposure": exposure, "priority": 0}}}
        ]
    }))
    .unwrap();
    fs::write(path, format!("{text}\n")).unwrap();
}

fn scratch(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("joy-volume-{}-{name}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    directory.join("level.joylevel")
}

fn overrides(record: Value) -> PostProcessOverrides {
    PostProcessOverrides::from_record(record.as_object().unwrap()).unwrap()
}

#[test]
fn saves_replace_only_the_volume_settings_and_detect_outside_edits() {
    let path = scratch("save");
    document(&path, 2.0);
    let mut file = VolumeFile::new(path.clone(), "look");
    let loaded = file.read().unwrap();
    assert_eq!(loaded.overrides.get("exposure"), Some(&json!(2.0)));

    let edited = overrides(json!({"bloomStrength": 0.4}));
    let revision = file.save(&edited, &loaded.revision).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.ends_with("}\n"));
    let saved: Value = serde_json::from_str(&text).unwrap();
    let volume = &saved["entities"][1]["components"]["postProcess"];
    assert_eq!(volume, &json!({"unbounded": true, "priority": 0, "bloomStrength": 0.4}));
    assert_eq!(saved["name"], "Test Level");
    assert!(file.poll().is_none(), "the editor's own save is not an outside edit");

    // A stale revision is refused with what is on disk now.
    document(&path, 3.0);
    match file.save(&edited, &revision) {
        Err(SaveError::Conflict(snapshot, _)) => assert_eq!(snapshot.overrides.get("exposure"), Some(&json!(3.0))),
        other => panic!("expected a conflict, got {other:?}"),
    }
    let leftovers = fs::read_dir(path.parent().unwrap()).unwrap().count();
    assert_eq!(leftovers, 1, "no temporary files remain");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn missing_volumes_and_invalid_documents_are_reported() {
    let path = scratch("invalid");
    document(&path, 2.0);
    assert!(VolumeFile::new(path.clone(), "crate").read().unwrap_err().contains("no postProcess"));
    fs::write(&path, "not json").unwrap();
    assert!(VolumeFile::new(path.clone(), "look").read().is_err());
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
    assert!(VolumeFile::new(path, "look").read().is_err());
}
