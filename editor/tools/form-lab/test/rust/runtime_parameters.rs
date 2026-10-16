// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! All five parameter types, resource overrides, compiled-script snapshots, and the geometry-only GLB mesh import,
//! ported from `test/runtime-parameters.test.js`.

use form_lab::document::read_document;
use form_lab::export_glb::export_glb;
use form_lab::parameter_values::{import_glb_mesh, read_parameters};
use joy_engine::form::CompiledScript;
use joy_engine::serde_json::{Value as Json, json};

use crate::support::compile;

const RECIPE: &str = "
param count = 3 meta(min=1, max=20, step=1)
param origin = vector(0, 0, 0) meta(min=-20, max=20, step=0.1)
param tint = rgba(0.3, 0.6, 0.9, 1)
param piece = meshBox(1, 2, 1)
param finish = materialPbr(tint, 0.1, 0.7)
repeat count as i {
  instance(piece, vector(origin.x + i * 2, origin.y, origin.z), vector(0, 0, 0), vector(1, 1, 1), finish)
}";

#[test]
fn one_recipe_accepts_all_five_parameter_types_and_round_trips_resource_overrides() {
    let initial = compile(RECIPE, json!({}), None);
    let kinds: Vec<&str> =
        read_parameters(&initial.parameters).iter().map(|parameter| parameter.kind().name()).collect();
    assert_eq!(kinds, ["scalar", "vector", "color", "mesh", "material"]);
    assert_eq!(initial.positions.len(), 3 * 8 * 3);
    let piece = compile("param piece = meshSphere(0.5, 6)", json!({}), None).parameters[0]["value"].clone();
    let overrides = json!({"count": 2, "origin": [4, 0, 0], "tint": [1, 0, 0, 1], "piece": piece});
    let document =
        read_document(&json!({"version": 1, "name": "Bridge", "source": RECIPE, "overrides": overrides}).to_string())
            .unwrap();
    let result = compile(&document.source, Json::Object(document.overrides.clone()), None);
    assert_eq!(result.triangles.len(), 2 * 24 * 3);
    let base_color: Vec<f64> =
        result.materials.last().unwrap()["baseColor"].as_array().unwrap().iter().filter_map(Json::as_f64).collect();
    assert_eq!(base_color, [1.0, 0.0, 0.0, 1.0]);
    assert!(export_glb(&result).unwrap().len() > 100);
    assert_eq!(compile(RECIPE, overrides.clone(), None), compile(RECIPE, overrides, None));
}

#[test]
fn mesh_manipulation_is_local_and_resource_boundaries_reject_malformed_geometry() {
    let result = compile(
        "let base = meshBox(2, 2, 2)
let bent = deform(base, x, y + x*x, z)
let moved = transformMesh(bent, vector(5,0,0), vector(0,0,0), vector(2,1,1))
instance(base)
instance(moved)",
        json!({}),
        None,
    );
    assert_eq!(&result.positions[..3], [-1.0, -1.0, -1.0]);
    assert_eq!(&result.positions[24..27], [3.0, 0.0, -1.0]);
    let invalid = json!({"kind": "mesh", "name": "Broken", "positions": [0, 0, 0], "normals": [0, 1, 0], "uvs": [0, 0],
        "indices": [0, 1, 2]});
    let error = joy_engine::form::compile(RECIPE, &json!({"piece": invalid}), None).unwrap_err();
    assert!(error.message.to_lowercase().contains("ind"), "{error}");
    let error = joy_engine::form::compile(RECIPE, &json!({"piece": 3}), None).unwrap_err();
    assert!(error.message.to_lowercase().contains("mesh"), "{error}");
    let error =
        joy_engine::form::compile("repeat 20000 as i { instance(meshBox(1,1,1)) }", &json!({}), None).unwrap_err();
    assert!(error.message.contains("limit") || error.message.contains("budget"), "{error}");
}

#[test]
fn a_parsed_runtime_recipe_evaluates_caller_snapshots_without_retaining_overrides_or_previous_output() {
    let program = CompiledScript::parse(&format!("project meta(name=\"Original\")\n{RECIPE}")).unwrap();
    let mut overrides = json!({"count": 1, "origin": [3, 0, 0]});
    let mut first = program.evaluate(&overrides, None).unwrap();
    first.project.as_mut().unwrap()["name"] = json!("Changed");
    assert_eq!(program.evaluate(&json!({}), None).unwrap().project.unwrap()["name"], "Original");
    overrides["origin"][0] = json!(8);
    assert_eq!(first.positions[0], 2.5);
    assert_eq!(program.evaluate(&overrides, None).unwrap().positions[0], 7.5);
    assert_eq!(program.evaluate(&json!({}), None).unwrap().positions.len(), 72);
}

#[test]
fn form_exported_glb_geometry_can_return_through_a_runtime_mesh_parameter() {
    let exported = export_glb(&compile("sphere(0,0,0,0.5,6)", json!({}), None)).unwrap();
    let piece = import_glb_mesh(&exported, "ball.glb").unwrap();
    assert_eq!(piece["name"], "ball.glb");
    assert_eq!(compile(RECIPE, json!({"piece": piece, "count": 1}), None).triangles.len(), 24 * 3);
    assert!(import_glb_mesh(b"not a glb", "broken.glb").is_err());
    assert!(import_glb_mesh(&vec![0; 16_000_001], "huge.glb").unwrap_err().contains("16 MB"));
}
