// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! GLB/glTF mesh decoding parity (strides, sparse accessors, embedded buffers, and a FORM LAB export), compared exactly
//! against the TypeScript engine.

use joy_dcc_gltf::{
    GlbError,
    parse_glb_mesh,
};
use joy_test_support::assert_matches_recording;
use serde_json::Value;

const GLB_REFERENCE: &str = include_str!("../fixtures/glb/glb-reference.json");
const MULTI_MESH_GLB: &[u8] = include_bytes!("../fixtures/glb/multi-mesh.glb");
const EMBEDDED_GLTF: &[u8] = include_bytes!("../fixtures/glb/embedded.gltf");
const FORM_SPHERE_GLB: &[u8] = include_bytes!("../fixtures/glb/form-sphere.glb");

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
