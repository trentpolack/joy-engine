// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/config_file.rs`.

use std::fs;

use super::*;
use crate::config_session::tests::fields;

#[test]
fn saves_only_overrides_and_refuse_outside_changes() {
    let directory = std::env::temp_dir().join(format!("joy-config-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("tuning-overrides.json");
    fs::write(&path, "{\n  \"$schema\": \"./tuning-overrides.schema.json\",\n  \"version\": 1,\n  \"values\": {\n    \"speed\": 8\n  }\n}\n").unwrap();

    let (mut file, mut values) = ConfigFile::open(path.clone(), &fields()).unwrap();
    assert_eq!(values["speed"], 8.0);
    assert_eq!(values["count"], 3.0);
    values.insert("speed".to_owned(), 4.0);
    values.insert("count".to_owned(), 5.0);
    file.save(&values).unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "{\n  \"$schema\": \"./tuning-overrides.schema.json\",\n  \"version\": 1,\n  \"values\": {\n    \"count\": 5\n  }\n}\n"
    );

    fs::write(&path, "{\"version\": 1, \"values\": {}}").unwrap();
    assert!(file.save(&values).unwrap_err().contains("changed outside"));
    fs::write(&path, "{\"version\": 2, \"values\": {}}").unwrap();
    assert!(ConfigFile::open(path, &fields()).is_err());
    fs::remove_dir_all(directory).unwrap();
}
