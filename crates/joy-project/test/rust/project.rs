// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Project manifest parity: every JoyGames `project.joyproject` manifest and the manifest validation rules, compared
//! exactly against the TypeScript engine.

use joy_project::{
    JOY_PROJECT_MANIFEST,
    parse_project_manifest,
    parse_project_manifest_text,
};
use joy_test_support::assert_matches_recording;
use serde_json::Value;

const MANIFEST_REFERENCE: &str = include_str!("../fixtures/project-manifest-reference.json");

/// Every JoyGames project manifest, copied verbatim.
const MANIFESTS: [(&str, &str); 8] = [
    ("blast-garden.joyproject", include_str!("../fixtures/projects/blast-garden.joyproject")),
    ("framebreak.joyproject", include_str!("../fixtures/projects/framebreak.joyproject")),
    ("god-game.joyproject", include_str!("../fixtures/projects/god-game.joyproject")),
    ("luna.joyproject", include_str!("../fixtures/projects/luna.joyproject")),
    ("magic-colors.joyproject", include_str!("../fixtures/projects/magic-colors.joyproject")),
    ("space-scuffles.joyproject", include_str!("../fixtures/projects/space-scuffles.joyproject")),
    ("vectors-to-victory.joyproject", include_str!("../fixtures/projects/vectors-to-victory.joyproject")),
    ("vehicle-playground.joyproject", include_str!("../fixtures/projects/vehicle-playground.joyproject")),
];

#[test]
fn every_joygames_manifest_parses_like_typescript() {
    let reference: Value = serde_json::from_str(MANIFEST_REFERENCE).unwrap();
    let recorded = reference["manifests"].as_array().unwrap();
    assert_eq!(recorded.len(), MANIFESTS.len());
    for ((name, text), expected) in MANIFESTS.iter().zip(recorded) {
        assert_eq!(expected["name"], *name);
        let manifest = parse_project_manifest_text(text).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_matches_recording(&manifest, &expected["result"]["ok"], name);
        assert!(!manifest.capabilities.is_empty(), "{name} declares editor capabilities");
    }
    assert_eq!(JOY_PROJECT_MANIFEST, "project.joyproject");
}

#[test]
fn manifest_validation_normalizes_and_rejects_like_typescript() {
    let reference: Value = serde_json::from_str(MANIFEST_REFERENCE).unwrap();
    for case in reference["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        match (parse_project_manifest(&case["value"]), case["result"].get("ok")) {
            (Ok(manifest), Some(expected)) => assert_matches_recording(&manifest, expected, name),
            (Err(error), None) => assert_eq!(error.to_string(), case["result"]["error"].as_str().unwrap(), "{name}"),
            (result, _) => panic!("{name}: {result:?} does not match {}", case["result"]),
        }
    }
    assert!(parse_project_manifest_text("{broken").is_err());
}
