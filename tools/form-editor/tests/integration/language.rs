// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The language as FORM EDITOR uses it and the geometry exports. OBJ and PLY text is also compared with reference exports
//! of the same build when a folder of them is configured.

use form_editor::export::{
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

/// A script that outputs `body` (statements that add points to `points`) as the output "Points".
fn points(body: &str) -> String {
    format!("let points = geometry()\n{body}\noutput(\"Points\", points)")
}

#[test]
fn expressions_lexical_loops_and_point_data() {
    let mesh = compile(
        &points("let size = 2 + 3 * 4\nrepeat 3 as i { addPoint(points, vector(i, size, sin(pi/2))) }"),
        json!({}),
        None,
    );
    assert_eq!(mesh.positions, [0.0, 14.0, 1.0, 1.0, 14.0, 1.0, 2.0, 14.0, 1.0]);
    assert_eq!(mesh.points.len(), 3);
}

#[test]
fn grids_shaped_in_a_wrangle_generate_indexed_geometry_with_evaluated_heights() {
    let mesh = compile(
        "let surface = grid(2, 2, 4, 4)
wrangle points in surface as point {
  point.position = vector(point.position.x, point.position.x + point.position.z, point.position.z)
}
output(\"Surface\", surface)",
        json!({}),
        None,
    );
    assert_eq!(mesh.positions.len(), 27);
    assert_eq!(mesh.triangles.len(), 24);
    assert_eq!(&mesh.positions[..3], [-2.0, -4.0, -2.0]);
}

#[test]
fn random_output_is_reproducible_and_independent_per_compile() {
    let script = points("seed(42)\nrepeat 10 as i { addPoint(points, vector(rand(), rand(), rand())) }");
    assert_eq!(compile(&script, json!({}), None), compile(&script, json!({}), None));
}

#[test]
fn nested_loops_preserve_outer_variables() {
    let mesh =
        compile(&points("repeat 2 as i { repeat 2 as j { addPoint(points, vector(i, j, 0)) } }"), json!({}), None);
    assert_eq!(mesh.positions, [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0]);
}

#[test]
fn reports_source_location_for_unknown_names_and_rejects_nonfinite_geometry() {
    let error = rejects(&points("addPoint(points, vector(0, 0, 0))\naddPoint(points, vector(missing, 0, 0))"));
    assert!(error.line == 3 && error.message.contains("Unknown"), "{error}");
    assert!(rejects(&points("addPoint(points, vector(1/0, 0, 0))")).message.contains("finite"));
}

#[test]
fn authored_normals_normalize_stay_per_point_and_shapes_keep_their_own_normals() {
    // A triangle with an authored normal on each corner, then a point without one.
    let mesh = compile(
        "let triangle = geometry()
let up = {normal: vector(0, 3, 4)}
addFace(triangle, addPoint(triangle, vector(0, 0, 0), up), addPoint(triangle, vector(1, 0, 0), up), addPoint(triangle, vector(0, 1, 0), up))
let dot = geometry()
addPoint(dot, vector(1, 2, 3))
output(\"Shape\", merge(triangle, dot))",
        json!({}),
        None,
    );
    assert_eq!(mesh.normals, [0.0, 0.6, 0.8, 0.0, 0.6, 0.8, 0.0, 0.6, 0.8, 0.0, 0.0, 0.0]);
    assert_eq!(compile(&points("addPoint(points, vector(0, 0, 0))"), json!({}), None).normals, [0.0, 0.0, 0.0]);
    for body in [
        "addPoint(points, vector(0, 0, 0), {normal: vector(1/0, 0, 0)})",
        "addPoint(points, vector(0, 0, 0), {normal: 3})",
    ] {
        let error = rejects(&points(body));
        assert!(error.line == 2 && (error.message.contains("normal") || error.message.contains("finite")), "{error}");
    }

    // Moved and placed spheres keep radial normals.
    for source in [
        "output(\"Ball\", translate(sphere(2, 8), vector(1, 2, 3)))",
        "let ball = sphere(2, 8)\ntranslate(ball, vector(1, 2, 3))\noutput(\"Ball\", ball)",
        "output(\"Ball\", place(meshSphere(2, 8), vector(1, 2, 3)))",
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

    // Merged parts keep their own normals: a smooth sphere does not lend its normals to a flat box.
    let following = compile("output(\"Parts\", merge(sphere(1, 8), box(1, 1, 1)))", json!({}), None);
    assert!(following.normals[following.normals.len() - 24..].iter().all(|value| *value == 0.0));
}

#[test]
fn rejects_arbitrary_javascript_oversized_work_invalid_topology_and_arity() {
    rejects("fetch(\"https://example.com\")");
    let message = rejects(&points("repeat 10000000 as i { addPoint(points, vector(0, 0, 0)) }")).message;
    assert!(message.contains("limit") || message.contains("between"), "{message}");
    let nested = points("repeat 1000 as i { repeat 1000 as j { addPoint(points, vector(i, j, 0)) } }");
    assert!(rejects(&nested).message.contains("limit"));
    assert!(rejects("output(\"Grid\", grid(0, 2, 4, 4))").message.contains("between"));
    assert!(rejects(&points("addPoint(points, vector(1, 2))")).message.contains("3 arguments"));
}

#[test]
fn obj_preserves_triangle_and_point_topology() {
    let text = export_obj(&compile(
        &points(
            "addPoint(points, vector(0, 0, 0))
addFace(points, addPoint(points, vector(0, 0, 0)), addPoint(points, vector(1, 0, 0)), addPoint(points, vector(0, 1, 0)))",
        ),
        json!({}),
        None,
    ));
    assert!(text.contains("p 1\n"));
    assert!(text.contains("f 2 3 4\n"));
    assert!(text.starts_with("# FORM LAB — Y-up, script units\no FormLab\nv 0 0 0\n"));
}

#[test]
fn ply_carries_per_vertex_colors_and_faces() {
    let text = export_ply(&compile("output(\"Box\", color(box(2, 2, 2), rgba(1, 0.5, 0, 1)))", json!({}), None));
    assert!(text.contains("element vertex 8\n"));
    assert!(text.contains("element face 12\n"));
    assert!(text.contains(" 255 128 0 255\n"), "PLY colors round half up");
}

#[test]
fn generated_json_lists_metadata_then_geometry_with_integral_numbers_as_integers() {
    let data =
        compile(&format!("project meta(name=\"A\")\n{}", points("addPoint(points, vector(1, 2, 3))")), json!({}), None);
    let text = export_geometry_json(&data).unwrap();
    assert!(text.starts_with("{\n  \"format\": \"form-editor-geometry\",\n  \"version\": 1,\n  \"upAxis\": \"Y\",\n"));
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
let points = geometry()
addPoint(points, vector(offset.x, offset.y * size, offset.z))
output(\"Points\", color(points, tint))",
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
    assert!(rejects("param p = vector(1,2,3) meta(min=0,max=5,step=1)\nlet x = p.r").message.contains("component"));
    let error =
        joy_engine::form::compile("param p = vector(1,2,3) meta(min=0,max=5,step=1)", &json!({"p": [1, 2]}), None)
            .unwrap_err();
    assert!(error.message.contains("components"), "{error}");
}

#[test]
fn project_metadata_travels_with_generated_geometry_and_rejects_duplicates() {
    let mesh = compile(
        "project meta(name=\"Wave study\", info=\"SURFACE / 001\", title=\"MAKE\\nWAVES.\")\noutput(\"Slab\", box(1, 1, 1))",
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
    let height_point = "let points = geometry()\naddPoint(points, vector(0, height, 0))";
    assert!(
        rejects(&format!("param height = 8 meta(min=0,max=5,step=1)\n{height_point}"))
            .message
            .contains("min ≤ default ≤ max")
    );
    let source = format!("param height = 2 meta(min=0,max=5,step=1)\n{height_point}");
    let output = "output(\"Points\", points)";
    assert_eq!(compile(&format!("{source}\n{output}"), json!({"height": 8}), None).positions, [0.0, 5.0, 0.0]);

    // A script-level `let` shadows the parameter from there on; the parameter itself stays read-only.
    assert_eq!(
        compile(
            &format!("{source}\nlet height = 9\naddPoint(points, vector(0, height, 0))\n{output}"),
            json!({}),
            None
        )
        .positions,
        [0.0, 2.0, 0.0, 0.0, 9.0, 0.0]
    );
    assert!(rejects(&format!("{source}\nheight = 9")).message.contains("parameter"));
}

#[test]
fn point_attribute_records_realize_per_vertex_survive_json_export_and_reject_nested_values() {
    let data = compile(
        "let home = {kind: \"home\", capacity: 2 + 2, enabled: true}
let homes = geometry()
addPoint(homes, vector(0, 0, 0), home)
addPoint(homes, vector(1, 0, 0), home)
let plain = geometry()
addPoint(plain, vector(2, 0, 0))
let trees = geometry()
addPoint(trees, vector(3, 0, 0), {kind: \"tree\", spot: vector(1, 2, 3)})
output(\"Village\", merge(homes, color(plain, rgba(0, 1, 0, 1)), trees))",
        json!({}),
        None,
    );
    let record = |index: usize| integral(&Json::Object(data.vertex_record(index)));
    let expected_home = json!({"capacity": 4, "enabled": true, "kind": "home", "spot": [0, 0, 0]});
    assert_eq!(record(0), expected_home);
    assert_eq!(record(1), expected_home);

    // Parts without an attribute hold its default.
    assert_eq!(record(2), json!({"capacity": 0, "enabled": false, "kind": "", "spot": [0, 0, 0]}));
    assert_eq!(record(3), json!({"capacity": 0, "enabled": false, "kind": "tree", "spot": [1, 2, 3]}));
    let exported: Json = serde_json::from_str(&export_geometry_json(&data).unwrap()).unwrap();
    assert_eq!(exported["vertexAttributes"]["kind"], json!({"type": "text", "values": ["home", "home", "", "tree"]}));
    assert_eq!(exported["vertexAttributes"]["capacity"], json!({"type": "number", "size": 1, "values": [4, 4, 0, 0]}));
    assert!(exported.get("userData").is_none());

    let single = |record: &str| points(&format!("addPoint(points, vector(0, 0, 0), {record})"));
    assert!(rejects(&single("3")).message.contains("record"));
    assert!(rejects(&single("{kind: \"home\", kind: \"tree\"}")).message.contains("Duplicate"));
    assert!(rejects(&single("{settings: {capacity: 4}}")).message.contains("records and lists"));
    assert!(rejects(&single("{tags: [\"village\", 7]}")).message.contains("records and lists"));
}

/// A value with integral numbers spelled as integers, for comparing evaluated records with literals.
fn integral(value: &Json) -> Json {
    joy_engine::core::integral_numbers_as_integers(value.clone())
}

/// OBJ and PLY text for the Material studio build matches the reference exports, except where FORM's transcendental
/// results differ in the last bits (positions are then compared within 1e-9).
#[test]
fn material_studio_obj_and_ply_match_the_reference_exports() {
    let Ok(reference) = std::env::var("FORM_EDITOR_REFERENCE_EXPORTS") else {
        eprintln!("skipping: set FORM_EDITOR_REFERENCE_EXPORTS to a folder of reference material-studio exports");
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
