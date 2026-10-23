// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Evaluation isolation, determinism, precedence, control flow, scopes, functions, modules, enums, parameters,
//! diagnostics, and budgets.

use joy_form::{
    CompiledScript,
    GeometryData,
    ParameterControl,
    compile,
    parameter_control,
    parameter_group,
    parameter_label,
};
use serde_json::{
    Value,
    json,
};
use std::collections::HashMap;

/// Evaluate `source`, which must output its result.
fn evaluate(source: &str) -> GeometryData {
    compile(source, &json!({}), None).unwrap_or_else(|error| panic!("{error} in {source:?}"))
}

/// Output each listed number as the X coordinate of one point, in order, and return them.
fn numbers(prelude: &str, values: &[&str]) -> Vec<f64> {
    let points: String = values.iter().map(|value| format!("addPoint(probe, vector({value}, 0, 0))\n")).collect();
    let data = evaluate(&format!("{prelude}\nlet probe = geometry()\n{points}output(\"Probe\", probe)"));
    data.positions.chunks(3).map(|position| position[0]).collect()
}

fn failure(source: &str) -> (String, usize, usize) {
    let error = compile(source, &json!({}), None).expect_err("expected an evaluation failure");
    (error.message, error.line, error.column)
}

#[test]
fn constants_preserve_values_scope_and_reject_replacement() {
    assert_eq!(
        numbers("const data = {height: 3}\nconst height = data.height + 1\nif true { let height = 8 }", &["height"]),
        vec![4.0]
    );
    for statement in
        ["value = 2", "value += 2", "let value = 2", "const value = 2", "param value = 2 meta(min=0,max=3,step=1)"]
    {
        let (message, line, _) = failure(&format!("const value = 1\n{statement}"));
        assert!(message.contains("constant"), "{statement}: {message}");
        assert_eq!(line, 2);
    }
    let (message, line, _) = failure("fn edit(ref value) { missing() }\nconst fixed = 1\nedit(ref fixed)");
    assert!(message.contains("constant"), "ref must reject before running missing(): {message}");
    assert_eq!(line, 3);
}

#[test]
fn constants_in_modules_freeze_captured_and_returned_geometry() {
    let loader = |_: &str| {
        Ok::<_, String>(
            "const shape = box(1,1,1)\nfn alias() { return shape }\nfn edit() { translate(shape,vector(1,0,0)) }"
                .into(),
        )
    };
    for operation in ["translate(kit.shape,vector(1,0,0))", "translate(kit.alias(),vector(1,0,0))", "kit.edit()"] {
        let source = format!("use \"kit.form\" as kit\n{operation}");
        let script = CompiledScript::parse_with_modules(&source, "main.form", &loader).unwrap();
        let error = script.evaluate(&json!({}), None).unwrap_err();
        assert!(error.message.contains("constant"), "{operation}: {error}");
        assert_eq!(error.line, 2);
    }
    let script =
        CompiledScript::parse_with_modules("use \"kit.form\" as kit\nconst alias = kit", "main.form", &loader).unwrap();
    assert!(script.evaluate(&json!({}), None).unwrap_err().message.contains("module"));
}

#[test]
fn range_loops_use_exclusive_bounds_and_evaluate_each_bound_once() {
    let source = r#"
        let trace = 0
        fn bound(ref trace, value) { trace = trace*10 + value return value }
        let sum = 0
        let end = 5
        for index in bound(ref trace,2)..bound(ref trace,end) { sum += index end = 0 }
        for index in -2..1 { sum += index }
        for index in 3..3 { sum = 999 }
        for index in 5..2 { sum = 999 }
        const decimal = 1. + .5 + 2e-1
    "#;
    assert_eq!(numbers(source, &["sum", "trace", "decimal"]), vec![6.0, 25.0, 1.7]);
    assert_eq!(numbers("let sum=0 for i in 9007199254740990..9007199254740991 { sum += 1 }", &["sum"]), vec![1.0]);
    for source in [
        "for i in 0..1.5 {}",
        "for i in -9007199254740992..0 {}",
        "for i in 0..9007199254740992 {}",
        "for i in 0..150001 {}",
    ] {
        let (message, _, _) = failure(source);
        assert!(message.contains("integer") || message.contains("iteration limit"), "{source}: {message}");
    }
    assert!(failure("for i in 0..1 { i = 3 }").0.contains("loop value"));
    for declaration in ["let", "const"] {
        assert!(failure(&format!("for i in 0..1 {{ {declaration} i = 3 }}")).0.contains("loop value"));
        assert!(failure(&format!("foreach item in [1] {{ {declaration} item = 3 }}")).0.contains("loop value"));
    }
    assert_eq!(numbers("let sum=0 for i in 2..4 { if true { let i=8 sum+=i } sum+=i }", &["sum"]), vec![21.0]);
    assert!(failure("for i in 0..1 {} let outside=i").0.contains("Unknown variable"));
}

#[test]
fn foreach_loops_visit_values_in_order_and_capture_the_collection_once() {
    let source = r#"
        let calls = 0
        fn source(ref calls) { calls += 1 return [{x:2}, {x:4}, {x:8}] }
        let ordered = 0
        foreach instance in source(ref calls) { ordered = ordered*10 + instance.x }
        let items = [1,2]
        let total = 0
        foreach item in items { total += item items = [99] }
        foreach item in [] { total = 999 }
        foreach nested in [[3,4], [5,6]] { foreach item in nested { total += item } }
        foreach component in vector(1,2,3) { total += component }
        foreach component in rgba(1,0,0,1) { total += component }
        let flags = 0
        foreach item in [true,"ok",null] {
            if flags == 0 { if item { flags += 1 } }
            else if flags == 1 { if item == "ok" { flags += 1 } }
            else { if item == null { flags += 1 } }
        }
    "#;
    assert_eq!(numbers(source, &["ordered", "calls", "total", "flags"]), vec![248.0, 1.0, 29.0, 3.0]);
    for expression in ["2", "true", "null", "\"text\"", "{x:1}", "geometry()"] {
        let (message, _, _) = failure(&format!("foreach item in {expression} {{}}"));
        assert!(message.contains("foreach expects"), "{expression}: {message}");
    }
    assert!(failure("foreach item in [1] { item = 2 }").0.contains("loop value"));
    assert!(failure("foreach item in [1] {} let outside=item").0.contains("Unknown variable"));
    assert_eq!(
        numbers("fn first(items) { foreach item in items { return item } return 0 }", &["first([7,9])"]),
        vec![7.0]
    );
}

#[test]
fn compiled_evaluations_are_owned_deterministic_and_ignore_unknown_overrides() {
    let source = "param width=2 meta(min=1,max=4,step=0.25)\nlet size=width+1\nlet g=geometry()\nfor i in 0..2 { addPoint(g, vector(size,i,rand())) }\noutput(\"Points\", g)";
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
    let values = numbers(
        "param a=2,0,10,1\nlet v=vector(1,2,3)*2+1",
        &[
            "-2^2",
            "2^3^2",
            "round(-1.5)",
            "v.x + v.y + v.z",
            "round(0.49999999999999994)",
            "round(9007199254740991)-9007199254740991",
            "a",
        ],
    );
    assert_eq!(values, vec![-4.0, 512.0, -1.0, 15.0, 0.0, 0.0, 2.0]);
    let data = compile(
        "param a=2,0,10,1\nlet g=geometry() addPoint(g,vector(a,0,0)) output(\"A\",g)",
        &json!({"a":3.2}),
        None,
    )
    .unwrap();
    assert_eq!(data.parameters[0]["value"], 3.2);
    assert_eq!(data.positions[0], 3.2);
}

#[test]
fn conditions_branch_on_true_or_false_and_logic_short_circuits() {
    let prelude = r#"
        let flags = 0
        for i in 0..6 {
            if i == 0 { flags += 1 } else if i % 2 == 0 AND NOT (i == 4) { flags += 10 } else { flags += 100 }
        }
        # The right side of AND or OR would fail if it ran.
        let skipped = false AND unknown_name
        let taken = true OR unknown_name
        let ordered = 1 < 2 AND 3 >= 3 OR false
    "#;
    let values =
        numbers(prelude, &["flags", "number(skipped)", "number(taken)", "number(ordered)", "number(NOT true)"]);
    assert_eq!(values, vec![1.0 + 10.0 + 400.0, 0.0, 1.0, 1.0, 0.0]);

    let (message, line, column) = failure("\nif 1 { }");
    assert_eq!(message, "Conditions require true or false; compare the number explicitly, such as 'count > 0'.");
    assert_eq!((line, column), (2, 4));
    assert_eq!(failure("let a = 1 AND true").0, "AND needs true or false on both sides; found a number.");
    assert_eq!(
        failure("let a = true + 1").0,
        "'+' needs numbers; true/false only supports ==, !=, AND, OR, and NOT. Use number(flag) for 1 or 0."
    );
    assert_eq!(failure("let a = \"tower\" == 3").0, "Cannot compare text with a number.");
}

#[test]
fn assignments_update_enclosing_variables_while_let_ends_with_its_block() {
    let prelude = r#"
        let total = 0
        let last = -1
        for i in 0..4 {
            let doubled = i * 2
            total += doubled
            last = i
        }
        let shadowed = 5
        if true { let shadowed = 9 shadowed = 7 }
    "#;
    assert_eq!(numbers(prelude, &["total", "last", "shadowed"]), vec![12.0, 3.0, 5.0]);

    assert_eq!(failure("for i in 0..2 { let inner = i }\nlet a = inner").0, "Unknown variable 'inner'.");
    assert_eq!(failure("x = 3").0, "Unknown variable 'x'; declare it with let before assigning it.");
    assert_eq!(failure("param w = 2 meta(min=1,max=3,step=1)\nw = 4").0, "Cannot assign to the parameter 'w'.");
    assert_eq!(failure("for i in 0..2 { i = 4 }").0, "Cannot assign to the loop value 'i'.");
    assert_eq!(failure("pi = 3").0, "Cannot assign to the constant 'pi'.");
}

#[test]
fn functions_copy_arguments_edit_ref_variables_and_return_values() {
    let prelude = r#"
        fn grow(target, amount) {
            addPoint(target, vector(amount, 0, 0))
            return pointCount(target)
        }
        fn grow_in_place(ref target, amount) {
            addPoint(target, vector(amount, 0, 0))
        }
        fn accumulate(ref sum, value) { sum += value }
        fn sign(value) {
            if value < 0 { return -1 }
            return 1
        }
        let base = geometry()
        addPoint(base, vector(0, 0, 0))
        let copied_count = grow(base, 1)
        grow_in_place(ref base, 2)
        let sum = 1
        accumulate(ref sum, 4)
    "#;
    let values = numbers(prelude, &["copied_count", "pointCount(base)", "sum", "sign(-3)", "sign(2)"]);
    assert_eq!(values, vec![2.0, 2.0, 5.0, -1.0, 1.0]);

    assert_eq!(failure("fn f() { let a = 1 }\nlet b = f()").0, "f() doesn't return a value.");
    assert_eq!(
        failure("fn f(ref a) { }\nlet b = 1\nf(b)").0,
        "Parameter 'a' of f is declared ref; pass a variable with 'ref a'."
    );
    assert_eq!(failure("fn f(a) { }\nlet b = 1\nf(ref b)").0, "Parameter 'a' of f is not declared ref; remove 'ref'.");
    assert_eq!(
        failure("let total = 0\nfn f() { total = 1 }\nf()").0,
        "Functions can't assign the script-level variable 'total'; return a value or pass it with ref."
    );
    assert_eq!(
        failure("fn box(a) { return a }").0,
        "Function 'box' would hide the built-in box(); choose another name."
    );
    assert_eq!(failure("fn f(n) { return f(n + 1) }\nlet a = f(0)").0, "Function call depth limit exceeded (32).");
}

#[test]
fn deep_function_calls_stay_within_a_test_thread_stack() {
    // Test threads have 2 MiB stacks; the deepest legal call chain, each with nested blocks, must fit.
    let nested = "if true { ".repeat(12) + "count += 1" + &" }".repeat(12);
    let source = format!(
        "fn descend(depth) {{\n let count = 0\n {nested}\n if depth > 0 {{ return descend(depth - 1) + count }}\n return count\n}}\nlet total = descend(30)\nlet g = geometry()\naddPoint(g, vector(total, 0, 0))\noutput(\"Total\", g)"
    );
    assert_eq!(evaluate(&source).positions[0], 31.0);
}

#[test]
fn enums_records_lists_and_text_compare_and_index() {
    let prelude = r#"
        enum Kind { tower, block, hall }
        let kind = Kind.hall
        let hit_points = {tower: 400, block: 250, hall: 900}
        let widths = [1.5, 2, 4]
        let label = "hall"
        let score = 0
        match kind {
            case Kind.tower, Kind.block { score = 1 }
            case Kind.hall { score = 2 }
            else { score = 3 }
        }
        let matched_text = 0
        if kind == label { matched_text = 1 }
    "#;
    let values = numbers(
        prelude,
        &["hit_points[kind]", "hit_points[\"tower\"]", "widths[2]", "itemCount(widths)", "score", "matched_text"],
    );
    assert_eq!(values, vec![900.0, 400.0, 4.0, 3.0, 2.0, 1.0]);

    assert_eq!(
        failure("enum A { x }\nenum B { x }\nlet c = A.x == B.x").0,
        "Cannot compare A member with B member; they belong to different enums."
    );
    assert_eq!(failure("enum A { x }\nlet c = A.y").0, "Enum A has no member 'y'.");
    assert_eq!(failure("let c = [1, 2][2]").0, "List index 2 is outside 0–1.");
}

#[test]
fn modules_share_functions_enums_and_constants_and_report_their_locations() {
    let modules: HashMap<&str, &str> = HashMap::from([
        (
            "city/parts/kit.form",
            "use \"../shared.form\" as shared\nenum Kind { tower, hall }\nlet floor_height = 3\nfn slab(width) { return box(width, shared.thickness, width) }\nfn broken() { return box(1, 1) }",
        ),
        ("city/shared.form", "let thickness = 0.25"),
    ]);
    let loader = |path: &str| modules.get(path).map(|source| source.to_string()).ok_or_else(|| "missing".to_owned());
    let source = "use \"parts/kit.form\" as kit\nlet slab = kit.slab(kit.floor_height)\naddAttribute(slab, \"points\", \"kind\", kit.Kind.hall)\noutput(\"Slab\", slab)";
    let script = CompiledScript::parse_with_modules(source, "city/block.form", &loader).unwrap();
    assert_eq!(script.module_paths(), vec!["city/parts/kit.form", "city/shared.form"]);
    let data = script.evaluate(&json!({}), None).unwrap();
    assert_eq!(&data.positions[..3], &[-1.5, -0.125, -1.5]);
    assert_eq!(data.vertex_record(0)["kind"], "hall");

    let error =
        CompiledScript::parse_with_modules("use \"parts/kit.form\" as kit\n\nkit.broken()", "city/block.form", &loader)
            .unwrap()
            .evaluate(&json!({}), None)
            .unwrap_err();
    assert_eq!(error.message, "city/parts/kit.form (5:22): box expects 3 arguments.");
    assert_eq!((error.line, error.column), (3, 1));

    let error = CompiledScript::parse("use \"kit.form\" as kit").unwrap_err();
    assert!(error.message.starts_with("Modules need a host that loads files"), "{error}");
    let error = CompiledScript::parse_with_modules("use \"missing.form\" as m", "a.form", &loader).unwrap_err();
    assert_eq!(error.message, "Could not load module 'missing.form': missing");
}

#[test]
fn retired_drawing_commands_explain_their_replacements() {
    for (source, start) in [
        ("color(1, 0, 0)", "color(r, g, b) no longer sets a drawing color"),
        ("box(0, 0, 0, 1, 1, 1)", "box(x, y, z, width, height, depth) was removed"),
        ("sphere(0, 0, 0, 1)", "sphere(x, y, z, radius) was removed"),
        ("point(0, 0, 0)", "point() was removed"),
        ("material(materialPbr(rgba(1,1,1,1), 0, 1))", "material(name) no longer selects a drawing material"),
        ("instance(meshBox(1,1,1))", "instance() was removed"),
        ("setUserData(geometry(), 0, {})", "User data was removed"),
        ("with userData({}) { }", "'with userData' blocks were removed"),
    ] {
        let message = match CompiledScript::parse(source) {
            Err(error) => error.message,
            Ok(script) => script.evaluate(&json!({}), None).unwrap_err().message,
        };
        assert!(message.starts_with(start), "{source}: {message}");
    }
}

#[test]
fn runtime_failures_retain_actual_expression_or_statement_locations() {
    let (message, line, column) = failure("\nlet a = unknown + 1");
    assert_eq!((message.as_str(), line, column), ("Unknown variable 'unknown'.", 2, 9));
    let (message, line, column) = failure("\nfor i in 0..\"three\" { }");
    assert_eq!((message.as_str(), line, column), ("Expected a number; found text.", 2, 1));
    let (message, _, _) = failure("for i in 0..150000 { for j in 0..150000 {} }");
    assert_eq!(message, "Operation limit exceeded (2,000,000).");
}

#[test]
fn maximum_flat_expression_chains_evaluate_and_drop_without_stack_growth() {
    let expression = vec!["1"; 19_000].join("+");
    assert_eq!(numbers(&format!("let sum = {expression}"), &["sum"]), vec![19_000.0]);
    let invalid = format!("let a = vector(1,2,3){}", ".x".repeat(10_000));
    assert!(compile(&invalid, &json!({}), None).is_err());
}

#[test]
fn parameter_presentation_describes_controls_and_whole_number_kinds_round_overrides() {
    let source = r#"
        param height = 2 meta(min=0, max=8, step=0.5, label="Tower height", group="Massing", info="Meters above grade.")
        param floors = 4 meta(min=1, max=12, kind="int")
        param lit = true
        param seed = 7 meta(kind="seed")
        param style = "Glass" meta(options=["Brick", "Glass", "Concrete"])
        let g = geometry()
        addPoint(g, vector(height, floors, number(lit)), {style: style})
        output("Probe", g)
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
        (lit["kind"].as_str(), lit["max"].as_f64(), lit["value"].as_f64(), lit["defaultValue"].as_f64()),
        (Some("bool"), Some(1.0), Some(0.0), Some(1.0))
    );
    assert_eq!(
        (seed["kind"].as_str(), seed["min"].as_f64(), seed["value"].as_f64()),
        (Some("seed"), Some(0.0), Some(12.0))
    );
    assert_eq!(style["kind"], "enum");
    assert_eq!(style["options"], json!(["Brick", "Glass", "Concrete"]));
    assert_eq!((style["defaultValue"].as_f64(), style["value"].as_f64()), (Some(1.0), Some(2.0)));
    assert_eq!(data.positions, vec![2.0, 7.0, 0.0]);
    assert_eq!(data.vertex_record(0)["style"], "Concrete");
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
    assert_eq!(
        failure("param x = \"Tile\" meta(options=[\"Brick\", \"Glass\"])"),
        "Parameter 'x' default \"Tile\" is not one of its options."
    );
}

#[test]
fn enum_and_text_parameters_read_as_their_own_types() {
    let source = r#"
        enum Roof { flat, gable, dome }
        param roof = Roof.gable
        param sign = "OPEN"
        let g = geometry()
        let shape = 0
        match roof {
            case Roof.flat { shape = 1 }
            case Roof.dome { shape = 3 }
            else { shape = 2 }
        }
        addPoint(g, vector(shape, 0, 0), {roof: roof, sign: sign})
        output("Probe", g)
    "#;
    let script = CompiledScript::parse(source).unwrap();
    let defaults = script.evaluate(&json!({}), None).unwrap();
    assert_eq!(defaults.positions[0], 2.0);
    assert_eq!(defaults.parameters[0]["options"], json!(["flat", "gable", "dome"]));
    assert_eq!(defaults.parameters[1]["type"], "text");
    let chosen = script.evaluate(&json!({"roof": "dome", "sign": "CLOSED"}), None).unwrap();
    assert_eq!(chosen.positions[0], 3.0);
    assert_eq!(chosen.parameters[0]["value"].as_f64(), Some(2.0));
    assert_eq!(chosen.vertex_record(0)["roof"], "dome");
    assert_eq!(chosen.vertex_record(0)["sign"], "CLOSED");
    let by_index = script.evaluate(&json!({"roof": 0}), None).unwrap();
    assert_eq!(by_index.positions[0], 1.0);
    assert_eq!(
        script.evaluate(&json!({"roof": "spire"}), None).unwrap_err().message,
        "Parameter 'roof' has no option \"spire\"."
    );
    assert_eq!(script.evaluate(&json!({"sign": 4}), None).unwrap_err().message, "Parameter 'sign' expects text.");
    assert_eq!(defaults.vertex_record(0).get("missing"), None::<&Value>);
}
