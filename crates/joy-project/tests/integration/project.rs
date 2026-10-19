// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Project manifests checked against `tests/fixtures/project-manifest-reference.json`, a golden recording of every
//! JoyGames `project.joyproject` manifest in its normalized document form and of the manifest validation rules.
//! Comparisons are exact. Re-record with `JOY_UPDATE_FIXTURES=1` when manifest parsing changes on purpose.

use joy_project::{
    JOY_PROJECT_MANIFEST,
    ProjectManifest,
    ProjectManifestError,
    parse_project_manifest,
    parse_project_manifest_text,
};
use joy_test_support::{
    Tolerance,
    golden_file,
};
use serde_json::{
    Value,
    json,
};

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

/// A parse result as the recording spells it: `{"ok": manifest}` in its document form, or `{"error": message}`.
fn result_record(result: &Result<ProjectManifest, ProjectManifestError>) -> Value {
    match result {
        Ok(manifest) => json!({"ok": serde_json::to_value(manifest).expect("manifests serialize")}),
        Err(error) => json!({"error": error.to_string()}),
    }
}

#[test]
fn every_joygames_manifest_parses_as_recorded() {
    let golden = golden_file!("project-manifest-reference.json");
    let results: Vec<_> = MANIFESTS.iter().map(|(_, text)| parse_project_manifest_text(text)).collect();
    let records: Vec<Value> = MANIFESTS
        .iter()
        .zip(&results)
        .map(|((name, _), result)| json!({"name": name, "result": result_record(result)}))
        .collect();
    golden.assert_section_matches("/manifests", &Value::Array(records), &Tolerance::exact());

    for ((name, _), result) in MANIFESTS.iter().zip(&results) {
        let declares_capabilities = result.as_ref().is_ok_and(|manifest| !manifest.capabilities.is_empty());
        assert!(declares_capabilities, "{name} declares editor capabilities");
    }
    assert_eq!(JOY_PROJECT_MANIFEST, "project.joyproject");
}

#[test]
fn manifest_validation_normalizes_and_rejects_as_recorded() {
    let golden = golden_file!("project-manifest-reference.json");
    let mut cases = golden.recording()["cases"].clone();
    for case in cases.as_array_mut().unwrap() {
        case["result"] = result_record(&parse_project_manifest(&case["value"]));
    }
    golden.assert_section_matches("/cases", &cases, &Tolerance::exact());
    assert!(parse_project_manifest_text("{broken").is_err());
}
