// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/project_manifest.rs`.

use super::*;
use serde_json::json;

#[test]
fn paths_normalize_and_reject_escapes() {
    assert_eq!(safe_relative_path("./dist\\site").unwrap(), "dist/site");
    assert!(safe_relative_path("/abs").is_err());
    assert!(safe_relative_path("C:/games").is_err());
    assert!(safe_relative_path("assets/../../x").is_err());
    assert!(is_identifier("particle-effects"));
    assert!(!is_identifier("Particle"));
    assert!(!is_identifier("1st"));
}

#[test]
fn minimal_manifests_omit_schema() {
    let minimal = json!({"schemaVersion": 1, "name": " Test ", "preview": {"root": "dist", "entry": "index.html"}, "capabilities": []});
    let manifest = parse_project_manifest(&minimal).unwrap();
    assert_eq!(manifest.name, "Test");
    let value = serde_json::to_value(&manifest).unwrap();
    assert_eq!(
        value.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["schemaVersion", "name", "preview", "capabilities"]
    );
}
