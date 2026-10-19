// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! GLB/glTF mesh decoding (strides, sparse accessors, embedded buffers, and a FORM LAB export) checked against
//! `tests/fixtures/glb/glb-reference.json`, a golden recording of each fixture mesh or its rejection message. Comparisons
//! are exact. Re-record with `JOY_UPDATE_FIXTURES=1` when decoding changes on purpose.

use joy_core::IndexedMesh3D;
use joy_dcc_gltf::{
    GlbError,
    parse_glb_mesh,
};
use joy_test_support::{
    Tolerance,
    golden_file,
};
use serde_json::{
    Value,
    json,
};

const MULTI_MESH_GLB: &[u8] = include_bytes!("../fixtures/glb/multi-mesh.glb");
const EMBEDDED_GLTF: &[u8] = include_bytes!("../fixtures/glb/embedded.gltf");
const FORM_SPHERE_GLB: &[u8] = include_bytes!("../fixtures/glb/form-sphere.glb");

/// The container a recorded case decodes, by its file name.
fn fixture_bytes(file: &str) -> &'static [u8] {
    match file {
        "multi-mesh.glb" => MULTI_MESH_GLB,
        "embedded.gltf" => EMBEDDED_GLTF,
        "form-sphere.glb" => FORM_SPHERE_GLB,
        other => panic!("unexpected fixture {other}"),
    }
}

/// A decode result as the recording spells it: `{"ok": mesh}` or `{"error": message}`.
fn result_record(result: Result<IndexedMesh3D, GlbError>) -> Value {
    match result {
        Ok(mesh) => json!({
            "ok": {
                "name": mesh.name,
                "positions": mesh.positions,
                "normals": mesh.normals,
                "indices": mesh.indices,
            }
        }),
        Err(error) => json!({"error": error.to_string()}),
    }
}

#[test]
fn glb_meshes_decode_as_recorded() {
    let golden = golden_file!("glb/glb-reference.json");
    let mut produced = golden.recording().clone();

    // Each case names a container and an optional mesh; `null` selects the first mesh.
    for case in produced.as_array_mut().unwrap() {
        let bytes = fixture_bytes(case["file"].as_str().unwrap());
        case["result"] = result_record(parse_glb_mesh(bytes, case["mesh"].as_str()));
    }
    golden.assert_matches(&produced, &Tolerance::exact());

    // An empty name selects the first mesh, the same as no name.
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
