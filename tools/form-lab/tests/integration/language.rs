// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The language as FORM LAB uses it and the geometry exports. OBJ and PLY text is also compared with reference exports
//! of the same build when a folder of them is configured.

use form_lab::export::{
    export_geometry_json,
    export_obj,
    export_ply,
};
use joy_engine::serde_json::{
    self,
    Value as Json,
    json,
};

use crate::support::compile;

fn rejects(source: &str) -> joy_engine::form::Diagnostic {
    joy_engine::form::compile(source, &json!({}), None).expect_err(source)
}

#[test]
fn expressions_lexical_loops_and_point_data() {
    let mesh = compile("let size = 2 + 3 * 4\nrepeat 3 as i { point(i, size, sin(pi/2)) }", json!({}), None);
    assert_eq!(mesh.positions, [0.0, 14.0, 1.0, 1.0, 14.0, 1.0, 2.0, 14.0, 1.0]);
    assert_eq!(mesh.points.len(), 3);
}

#[test]
fn surface_generates_indexed_geometry_with_evaluated_heights() {
    let mesh = compile("surface(2, 2, 4, 4, x+z)", json!({}), None);
    assert_eq!(mesh.positions.len(), 27);
    assert_eq!(mesh.triangles.len(), 24);
    assert_eq!(&mesh.positions[..3], [-2.0, -4.0, -2.0]);
}

#[test]
fn random_output_is_reproducible_and_independent_per_compile() {
    let script = "seed(42)\nrepeat 10 as i { point(rand(),rand(),rand()) }";
    assert_eq!(compile(script, json!({}), None), compile(script, json!({}), None));
}

#[test]
fn nested_loops_preserve_outer_variables() {
    let mesh = compile("repeat 2 as i { repeat 2 as j { point(i,j,0) } }", json!({}), None);
    assert_eq!(mesh.positions, [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0]);
}

#[test]
fn reports_source_location_for_unknown_names_and_rejects_nonfinite_geometry() {
    let error = rejects("point(0,0,0)\npoint(missing,0,0)");
    assert!(error.line == 2 && error.message.contains("Unknown"), "{error}");
    assert!(rejects("point(1/0,0,0)").message.contains("finite"));
}

#[test]
fn authored_normals_normalize_persist_reset_to_flat_and_remain_isolated_per_build() {
    let mesh = compile("normal(0,3,4) triangle(0,0,0,1,0,0,0,1,0) normal() point(1,2,3)", json!({}), None);
    assert_eq!(mesh.normals, [0.0, 0.6, 0.8, 0.0, 0.6, 0.8, 0.0, 0.6, 0.8, 0.0, 0.0, 0.0]);
    assert_eq!(compile("point(0,0,0)", json!({}), None).normals, [0.0, 0.0, 0.0]);
    for source in ["normal(0,0,0)", "normal(1,2)", "normal(1/0,0,0)"] {
        let error = rejects(source);
        assert!(error.line == 1 && (error.message.contains("normal") || error.message.contains("finite")), "{error}");
    }
    for source in [
        "sphere(1,2,3,2,8)",
        "let g=sphere(2,8) translate(g,vector(1,2,3)) output(\"Ball\",g)",
        "instance(meshSphere(2,8),vector(1,2,3))",
    ] {
        let sphere = compile(source, json!({}), None);
        for index in (0..sphere.positions.len()).step_by(3) {
            let normal = &sphere.normals[index..index + 3];
            assert!((joy_engine::math::DVec3::from_slice(normal).length() - 1.0).abs() < 1e-12);
            for axis in 0..3 {
                let expected = (sphere.positions[index + axis] - [1.0, 2.0, 3.0][axis]) / 2.0;
                assert!((normal[axis] - expected).abs() < 1e-12, "{source}");
            }
        }
    }
    let following = compile("sphere(0,0,0,1,8) box(0,0,0,1,1,1)", json!({}), None);
    assert!(following.normals[following.normals.len() - 24..].iter().all(|value| *value == 0.0));
    let preserved = compile("normal(0,3,4) sphere(0,0,0,1,8) point(1,2,3)", json!({}), None);
    assert_eq!(&preserved.normals[preserved.normals.len() - 3..], [0.0, 0.6, 0.8]);
}

#[test]
fn rejects_arbitrary_javascript_oversized_work_invalid_topology_and_arity() {
    rejects("fetch(\"https://example.com\")");
    let message = rejects("repeat 10000000 as i { point(0,0,0) }").message;
    assert!(message.contains("limit") || message.contains("between"), "{message}");
    assert!(rejects("repeat 1000 as i { repeat 1000 as j { point(i,j,0) } }").message.contains("limit"));
    assert!(rejects("surface(0,2,4,4,0)").message.contains("between"));
    assert!(rejects("point(1,2)").message.contains("3 arguments"));
}

#[test]
fn obj_preserves_triangle_and_point_topology() {
    let text = export_obj(&compile("point(0,0,0)\ntriangle(0,0,0,1,0,0,0,1,0)", json!({}), None));
    assert!(text.contains("p 1\n"));
    assert!(text.contains("f 2 3 4\n"));
    assert!(text.starts_with("# FORM LAB — Y-up, script units\no FormLab\nv 0 0 0\n"));
}

#[test]
fn ply_carries_per_vertex_colors_and_faces() {
    let text = export_ply(&compile("color(1,0.5,0)\nbox(0,0,0,2,2,2)", json!({}), None));
    assert!(text.contains("element vertex 8\n"));
    assert!(text.contains("element face 12\n"));
    assert!(text.contains(" 255 128 0 255\n"), "PLY colors round half up");
}

#[test]
fn generated_json_lists_metadata_then_geometry_with_integral_numbers_as_integers() {
    let data = compile("project meta(name=\"A\")\npoint(1,2,3)", json!({}), None);
    let text = export_geometry_json(&data).unwrap();
    assert!(text.starts_with("{\n  \"format\": \"form-lab-geometry\",\n  \"version\": 1,\n  \"upAxis\": \"Y\",\n"));
    let value: Json = serde_json::from_str(&text).unwrap();
    assert_eq!(value["positions"], json!([1, 2, 3]));
    assert_eq!(value["project"]["name"], "A");
    assert!(!text.contains("1.0"), "integral numbers print without a decimal point");
}

#[test]
fn named_metadata_and_typed_parameters_generate_geometry_from_components() {
    let mesh = compile(
        "param size = 2 meta(step=0.1, max=5, min=1)
param offset = vector(1, 2, 3) meta(min=-5, max=5, step=0.1)
param tint = rgba(1, 0.5, 0, 0.25)
color(tint)
point(offset.x, offset.y * size, offset.z)",
        json!({"offset": [3, 4, 99]}),
        None,
    );
    assert_eq!(mesh.positions, [3.0, 8.0, 5.0]);
    assert_eq!(mesh.colors, [1.0, 0.5, 0.0]);
    assert_eq!(mesh.alphas, [0.25]);
    let kinds: Vec<&str> = mesh.parameters.iter().map(|parameter| parameter["type"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["scalar", "vector", "color"]);
    assert_eq!(mesh.parameters[1]["defaultValue"].as_array().unwrap().len(), 3);
}

#[test]
fn typed_declarations_reject_invalid_metadata_components_and_override_shapes() {
    assert!(rejects("param p = 1 meta(min=0, min=1, max=2, step=1)").message.contains("Duplicate"));
    assert!(rejects("param p = 1 meta(low=0, max=2, step=1)").message.contains("metadata"));
    assert!(rejects("param p = vector(1,2,3) meta(min=0,max=5,step=1) point(p.r,0,0)").message.contains("component"));
    let error =
        joy_engine::form::compile("param p = vector(1,2,3) meta(min=0,max=5,step=1)", &json!({"p": [1, 2]}), None)
            .unwrap_err();
    assert!(error.message.contains("components"), "{error}");
}

#[test]
fn project_metadata_travels_with_generated_geometry_and_rejects_duplicates() {
    let mesh = compile(
        "project meta(name=\"Wave study\", info=\"SURFACE / 001\", title=\"MAKE\\nWAVES.\")\npoint(0,0,0)",
        json!({}),
        None,
    );
    assert_eq!(mesh.project, Some(json!({"name": "Wave study", "info": "SURFACE / 001", "title": "MAKE\nWAVES."})));
    for source in [
        "project meta(name=\"A\") project meta(name=\"B\")",
        "project meta(name=\"A\", name=\"B\")",
        "project meta(name=12)",
        "project meta(name=\"A\", unknown=\"B\")",
        "repeat 1 as i { project meta(name=\"A\") }",
    ] {
        rejects(source);
    }
}

#[test]
fn parameter_bounds_validate_script_defaults_and_clamp_overrides() {
    assert!(
        rejects("param height = 8 meta(min=0,max=5,step=1) point(0,height,0)").message.contains("min ≤ default ≤ max")
    );
    let source = "param height = 2 meta(min=0,max=5,step=1) point(0,height,0)";
    assert_eq!(compile(source, json!({"height": 8}), None).positions, [0.0, 5.0, 0.0]);
    assert_eq!(
        compile(&format!("{source}\nlet height = 9\npoint(0,height,0)"), json!({}), None).positions,
        [0.0, 2.0, 0.0, 0.0, 9.0, 0.0]
    );
}

#[test]
fn user_data_survives_emissions_and_json_export_and_scopes_restore() {
    let data = compile(
        "let home = {kind: \"home\", settings: {capacity: 2 + 2, enabled: true}, tags: [\"village\", 7, null]}
userData(home)
point(0, 0, 0)
color(0, 1, 0)
point(1, 0, 0)
userData()
point(2, 0, 0)
userData({kind: \"tree\", position: vector(1, 2, 3)})
point(3, 0, 0)",
        json!({}),
        None,
    );
    let records = data.user_data.as_ref().unwrap();
    let expected = json!({"kind": "home", "settings": {"capacity": 4, "enabled": true}, "tags": ["village", 7, null]});
    assert_eq!(integral(&records[0]), expected);
    let exported: Json = serde_json::from_str(&export_geometry_json(&data).unwrap()).unwrap();
    assert_eq!(exported["userData"][1], expected);
    assert_eq!(records[2], Json::Null);
    assert_eq!(integral(&records[3]), json!({"kind": "tree", "position": [1, 2, 3]}));
    assert!(rejects("with userData(3) { point(0,0,0) }").message.contains("record"));
    assert!(rejects("userData({kind: \"home\", kind: \"tree\"})").message.contains("Duplicate"));
}

/// A value with integral numbers spelled as integers, for comparing evaluated records with literals.
fn integral(value: &Json) -> Json {
    form_lab::export::integral_numbers_as_integers(value.clone())
}

/// OBJ and PLY text for the Material studio build matches the reference exports, except where FORM's transcendental
/// results differ in the last bits (positions are then compared within 1e-9).
#[test]
fn material_studio_obj_and_ply_match_the_reference_exports() {
    let Ok(reference) = std::env::var("FORM_LAB_REFERENCE_EXPORTS") else {
        eprintln!("skipping: set FORM_LAB_REFERENCE_EXPORTS to a folder of reference material-studio exports");
        return;
    };
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/examples/material-studio.form"),
    )
    .unwrap();
    let data = compile(&source, json!({}), None);
    for (name, text) in [("material-studio.obj", export_obj(&data)), ("material-studio.ply", export_ply(&data))] {
        let expected = std::fs::read_to_string(std::path::Path::new(&reference).join(name)).unwrap();
        let (ours, theirs): (Vec<&str>, Vec<&str>) = (text.lines().collect(), expected.lines().collect());
        assert_eq!(ours.len(), theirs.len(), "{name}");
        for (line, (left, right)) in ours.iter().zip(&theirs).enumerate() {
            if left == right {
                continue;
            }
            let numbers = |text: &str| -> Vec<f64> { text.split(' ').filter_map(|part| part.parse().ok()).collect() };
            let (left_numbers, right_numbers) = (numbers(left), numbers(right));
            assert_eq!(left_numbers.len(), right_numbers.len(), "{name}:{line}: {left} vs {right}");
            for (a, b) in left_numbers.iter().zip(&right_numbers) {
                assert!((a - b).abs() <= 1e-9 * a.abs().max(1.0), "{name}:{line}: {left} vs {right}");
            }
        }
    }
}
