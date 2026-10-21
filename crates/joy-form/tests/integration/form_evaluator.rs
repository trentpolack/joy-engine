// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Evaluation isolation, determinism, precedence, geometry commands, scoped user data, diagnostics, and budgets.

use joy_form::{
    CompiledScript,
    ParameterControl,
    compile,
    parameter_control,
    parameter_group,
    parameter_label,
};
use serde_json::json;

#[test]
fn compiled_evaluations_are_owned_deterministic_and_ignore_unknown_overrides() {
    let source = "param width=2 meta(min=1,max=4,step=0.25) let width=width+1 repeat 2 as i { point(width,i,rand()) }";
    let script = CompiledScript::parse(source).unwrap();
    let mut first = script.evaluate(&json!({"width":100,"removed":999}), None).unwrap();
    let second = script.evaluate(&json!({"width":100}), None).unwrap();
    assert_eq!(first.positions, second.positions);
    assert_eq!(first.positions[0], 5.0);
    assert_eq!(first.positions[2], 1015568748.0 / 4294967296.0);
    assert_eq!(first.parameters[0]["value"], 4.0);
    assert_eq!(first.parameters[0]["defaultValue"], 2.0);
    first.positions[0] = -100.0;
    assert_eq!(second.positions[0], 5.0);
    let empty = compile("", &json!({}), None).unwrap();
    assert!(empty.positions.is_empty());
    assert_eq!(empty.materials.len(), 1);
}

#[test]
fn actual_form_precedence_vector_math_and_legacy_parameters_are_preserved() {
    let data = compile(
        "param a=2,0,10,1 point(-2^2,2^3^2,round(-1.5)) let v=vector(1,2,3)*2+1 point(v.x,v.y,v.z) point(round(0.49999999999999994),round(9007199254740991)-9007199254740991,0)",
        &json!({"a":3.2}),
        None,
    )
    .unwrap();
    assert_eq!(data.positions, vec![-4.0, 512.0, -1.0, 3.0, 5.0, 7.0, 0.0, 0.0, 0.0]);
    assert_eq!(data.parameters[0]["value"], 3.2);
}

#[test]
fn geometry_commands_surface_deform_and_resource_instances_share_ts_layout() {
    let source = "let m=deform(meshBox(2,2,2),x,y+1,z) instance(m,vector(3,0,0),vector(0,0,0),vector(-1,1,1)) surface(1,1,2,2,x+z)";
    let data = compile(source, &json!({}), None).unwrap();
    assert_eq!(data.positions.len(), 12 * 3);
    assert_eq!(data.triangles.len(), 14 * 3);
    assert_eq!(&data.positions[..3], &[4.0, 0.0, -1.0]);
    assert_eq!(&data.triangles[..3], &[0, 1, 2]);
    assert_eq!(&data.positions[24..], &[-1.0, -2.0, -1.0, 1.0, 0.0, -1.0, -1.0, 0.0, 1.0, 1.0, 2.0, 1.0]);
}

#[test]
fn scoped_user_data_restores_and_resource_handles_are_not_authored_data() {
    let source = "userData({kind:\"outer\"}) with userData({kind:\"inner\",tags:[true,null,vector(1,2,3)]}) { point(0,0,0) userData({kind:\"changed\"}) } point(1,0,0) userData() point(2,0,0)";
    let data = compile(source, &json!({}), None).unwrap();
    let user = data.user_data.unwrap();
    assert_eq!(user[0]["kind"], "inner");
    assert_eq!(user[1]["kind"], "outer");
    assert_eq!(user[2], serde_json::Value::Null);
    assert!(compile("userData({mesh:meshBox(1,1,1)})", &json!({}), None).is_err());
}

#[test]
fn runtime_failures_retain_actual_expression_or_statement_locations() {
    let error = compile("\npoint(unknown,0,0)", &json!({}), None).unwrap_err();
    assert_eq!((error.line, error.column), (2, 7));
    assert_eq!(error.message, "Unknown variable 'unknown'.");
    let error = compile("\nif true { point(0,0,0) }", &json!({}), None).unwrap_err();
    assert_eq!((error.line, error.column), (2, 1));
    assert!(error.message.contains("finite scalar"));
    let error = compile("repeat 150000 as i { repeat 150000 as j {} }", &json!({}), None).unwrap_err();
    assert_eq!(error.message, "Operation limit exceeded (2,000,000).");
}

#[test]
fn maximum_flat_expression_chains_evaluate_and_drop_without_stack_growth() {
    let expression = vec!["1"; 19_000].join("+");
    let source = format!("point({expression},0,0)");
    let data = compile(&source, &json!({}), None).unwrap();
    assert_eq!(data.positions, vec![19_000.0, 0.0, 0.0]);
    let invalid = format!("point(vector(1,2,3){},0,0)", ".x".repeat(10_000));
    assert!(compile(&invalid, &json!({}), None).is_err());
}

#[test]
fn parameter_presentation_describes_controls_and_whole_number_kinds_round_overrides() {
    let source = r#"
        param height = 2 meta(min=0, max=8, step=0.5, label="Tower height", group="Massing", info="Meters above grade.")
        param floors = 4 meta(min=1, max=12, kind="int")
        param lit = 1 meta(kind="bool")
        param seed = 7 meta(kind="seed")
        param style = 1 meta(options=["Brick", "Glass", "Concrete"])
        point(height, floors, style)
    "#;
    let script = CompiledScript::parse(source).unwrap();
    let data = script.evaluate(&json!({"floors": 6.6, "lit": false, "seed": 12.4, "style": 9}), None).unwrap();
    let [height, floors, lit, seed, style] = &data.parameters[..] else {
        panic!("expected five parameter descriptors");
    };
    assert_eq!(height["label"], "Tower height");
    assert_eq!(height["group"], "Massing");
    assert_eq!(height["info"], "Meters above grade.");
    assert!(height.get("kind").is_none());
    assert_eq!(
        (floors["kind"].as_str(), floors["step"].as_f64(), floors["value"].as_f64()),
        (Some("int"), Some(1.0), Some(7.0))
    );
    assert_eq!(
        (lit["kind"].as_str(), lit["max"].as_f64(), lit["value"].as_f64()),
        (Some("bool"), Some(1.0), Some(0.0))
    );
    assert_eq!(
        (seed["kind"].as_str(), seed["min"].as_f64(), seed["value"].as_f64()),
        (Some("seed"), Some(0.0), Some(12.0))
    );
    assert_eq!(style["kind"], "enum");
    assert_eq!(style["options"], json!(["Brick", "Glass", "Concrete"]));
    assert_eq!(style["value"].as_f64(), Some(2.0));
    assert_eq!(data.positions, vec![2.0, 7.0, 2.0]);
    assert_eq!(parameter_label(height), "Tower height");
    assert_eq!(parameter_group(height), Some("Massing"));
    assert_eq!(parameter_control(floors), ParameterControl::Integer);
    assert_eq!(parameter_control(lit), ParameterControl::Toggle);
    assert_eq!(parameter_control(seed), ParameterControl::Seed);
    assert_eq!(
        parameter_control(style),
        ParameterControl::Choice(vec!["Brick".into(), "Glass".into(), "Concrete".into()])
    );

    let failure = |source: &str| compile(source, &json!({}), None).unwrap_err().message;
    assert_eq!(
        failure("param x = 1.5 meta(min=0, max=4, kind=\"int\")"),
        "Int, bool, seed, and enum parameters require whole-number defaults, bounds, and steps."
    );
    assert_eq!(
        failure("param x = 0 meta(min=0, max=1, kind=\"bool\")"),
        "Boolean and option parameters derive their bounds; remove min, max, and step."
    );
    assert_eq!(failure("param x = 0 meta(kind=\"enum\")"), "Enum parameters require options.");
    assert_eq!(failure("param x = 1 meta(kind=\"float\")"), "Parameter kind 'float' must be int, bool, seed, or enum.");
}
