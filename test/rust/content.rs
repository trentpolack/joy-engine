// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Content-layer parity: every JoyGames `project.joyproject` manifest and the manifest validation rules, and GLB/glTF
//! mesh decoding (strides, sparse accessors, embedded buffers, and a FORM LAB export), compared exactly against the
//! TypeScript engine.

use joy_engine::dcc::{
    GlbError,
    parse_glb_mesh,
};
use joy_engine::project::{
    JOY_PROJECT_MANIFEST,
    parse_project_manifest,
    parse_project_manifest_text,
};
use serde_json::Value;

use crate::parity_support::assert_matches_recording;

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

const GLB_REFERENCE: &str = include_str!("../fixtures/glb/glb-reference.json");
const MULTI_MESH_GLB: &[u8] = include_bytes!("../fixtures/glb/multi-mesh.glb");
const EMBEDDED_GLTF: &[u8] = include_bytes!("../fixtures/glb/embedded.gltf");
const FORM_SPHERE_GLB: &[u8] = include_bytes!("../fixtures/glb/form-sphere.glb");

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

#[test]
fn glb_meshes_decode_like_the_typescript_loader() {
    let reference: Value = serde_json::from_str(GLB_REFERENCE).unwrap();
    for case in reference.as_array().unwrap() {
        let bytes = match case["file"].as_str().unwrap() {
            "multi-mesh.glb" => MULTI_MESH_GLB,
            "embedded.gltf" => EMBEDDED_GLTF,
            "form-sphere.glb" => FORM_SPHERE_GLB,
            other => panic!("unexpected fixture {other}"),
        };
        let context = format!("{} {}", case["file"], case["mesh"]);
        match (parse_glb_mesh(bytes, case["mesh"].as_str()), case["result"].get("ok")) {
            (Ok(mesh), Some(expected)) => assert_matches_recording(&mesh, expected, &context),
            (Err(error), None) => assert_eq!(error.to_string(), case["result"]["error"].as_str().unwrap(), "{context}"),
            (result, _) => panic!("{context}: {result:?} does not match {}", case["result"]),
        }
    }

    // An empty name selects the first mesh, like the TypeScript loader.
    assert_eq!(parse_glb_mesh(MULTI_MESH_GLB, Some("")).unwrap().name, "Tetra");
}

#[test]
fn malformed_containers_and_external_buffers_fail_without_panicking() {
    // Every truncation of a valid container is rejected cleanly.
    for length in [0, 3, 11, 12, 19, 20, 64, MULTI_MESH_GLB.len() - 1] {
        assert!(parse_glb_mesh(&MULTI_MESH_GLB[..length], None).is_err(), "length {length}");
    }

    let mut wrong_version = MULTI_MESH_GLB.to_vec();
    wrong_version[4] = 1;
    assert_eq!(parse_glb_mesh(&wrong_version, None), Err(GlbError::UnsupportedVersion));

    let external = br#"{"asset": {"version": "2.0"}, "buffers": [{"byteLength": 4, "uri": "mesh.bin"}],
        "bufferViews": [{"buffer": 0, "byteLength": 4}],
        "accessors": [{"bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3"}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 0}}]}]}"#;
    assert_eq!(parse_glb_mesh(external, None), Err(GlbError::ExternalBuffer("mesh.bin".to_owned())));

    let compressed =
        br#"{"asset": {"version": "2.0"}, "extensionsRequired": ["KHR_draco_mesh_compression"], "meshes": []}"#;
    assert_eq!(
        parse_glb_mesh(compressed, None),
        Err(GlbError::UnsupportedExtension("KHR_draco_mesh_compression".to_owned()))
    );
    assert_eq!(parse_glb_mesh(br#"{"asset": {"version": "2.0"}}"#, None), Err(GlbError::MissingMesh));
    assert!(matches!(parse_glb_mesh(b"not json", None), Err(GlbError::Json(_))));

    let out_of_bounds = br#"{"asset": {"version": "2.0"},
        "buffers": [{"byteLength": 4, "uri": "data:application/octet-stream;base64,AAAAAA=="}],
        "bufferViews": [{"buffer": 0, "byteLength": 4}],
        "accessors": [{"bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3"}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 0}}]}]}"#;
    assert!(matches!(parse_glb_mesh(out_of_bounds, None), Err(GlbError::InvalidAccessor(_))));
}
