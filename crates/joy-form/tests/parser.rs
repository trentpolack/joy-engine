// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Parser grammar, UTF-16 diagnostics, metadata validation, and source budgets.

use joy_form::{CompiledScript, compile};
use serde_json::json;

fn assert_error(source: &str, message: &str, line: usize, column: usize) {
    let error = match CompiledScript::parse(source) {
        Ok(_) => panic!("Expected parse failure for {source:?}"),
        Err(error) => error,
    };
    assert_eq!(error.message, message, "source: {source:?}");
    assert_eq!((error.line, error.column), (line, column));
}

#[test]
fn parses_complete_statement_and_data_syntax() {
    let source = r#"
        # Headers and semicolons can separate statements.
        project meta(name="Parser study", title="A \"title\"", info="line\nline");;
        param width = 2 meta(min=0, max=10, step=.25)
        param height = 3, 0, 10, 1
        let data = {"display name": "hello", nested: [true, false, null, {}, []]}
        let g = geometry()
        with userData(data) {
            repeat width as i {
                if i >= 1 { point(i, 0, 0) }
            }
        }
        wrangle points in g as p {
            p.position = vector(p.position.x + 1, 0, 0)
        }
        // Names are interpreted as commands only during evaluation.
        arbitrary(1e-2, 5., +3, (width + height))
    "#;
    assert!(CompiledScript::parse(source).is_ok());
}

#[test]
fn exponentiation_binds_more_tightly_than_unary_and_associates_right() {
    let geometry = compile("point(-2^2, 2^3^2, 7%4+8/2*3)", &json!({}), None).unwrap();
    assert_eq!(geometry.positions, vec![-4.0, 512.0, 15.0]);
}

#[test]
fn reports_utf16_columns_and_comment_line_boundaries() {
    assert_error(
        "\"😀\"; let x = @",
        "@attribute syntax is no longer supported. Use 'wrangle points in geometry as point' and 'point.name'; declare fields first with addAttribute().",
        1,
        15,
    );
    assert_error("#😀\r\n\"😀\"; let x = ;", "Expected an expression, found ';'.", 2, 15);
    assert_error("\u{feff}\u{00a0}let x = )", "Expected an expression, found ')'.", 1, 11);
    assert_error("\u{0085}", "Unexpected character '\u{0085}'.", 1, 1);
}

#[test]
fn rejects_invalid_metadata_and_data_keys_at_the_original_tokens() {
    assert_error("project meta(title=\"T\")", "Project metadata requires a name.", 1, 1);
    assert_error("project meta(name=\"\")", "Project name requires nonempty, single-line text.", 1, 19);
    assert_error("project meta(name=1)", "Project metadata requires double-quoted text.", 1, 19);
    assert_error("project meta(name=\"X\", name=\"Y\")", "Duplicate project metadata 'name'.", 1, 24);
    assert_error("project meta(other=\"X\")", "Unknown project metadata 'other'.", 1, 14);
    assert_error("param x = 1 meta(min=0,min=2)", "Duplicate metadata 'min'.", 1, 27);
    assert_error("param x = 1 meta(label=2)", "Unknown parameter metadata 'label'.", 1, 23);
    assert_error("let x = {a:1,a:2}", "Duplicate user data field 'a'.", 1, 14);
    assert_error(
        "let x = {constructor:1}",
        "Invalid user data field name (1–64 characters; prototype keys are reserved).",
        1,
        10,
    );
    assert_error(
        "let x = {\"\":1}",
        "Invalid user data field name (1–64 characters; prototype keys are reserved).",
        1,
        10,
    );
}

#[test]
fn rejects_trailing_commas_and_unterminated_blocks() {
    assert_error("f(1,)", "Expected an expression, found ')'.", 1, 5);
    assert_error("let x = [1,]", "Expected an expression, found ']'.", 1, 12);
    assert_error("let x = {a:1,}", "Expected a variable name.", 1, 14);
    assert_error("repeat 2 as i {", "Expected '}', found '<end>'.", 1, 16);
    assert_error("}", "Unexpected '}'.", 1, 1);
    assert_error("let x = \"bad\\q\"", "Invalid quoted text.", 1, 9);
    assert_error("let x = \"line\nline\"", "Unexpected character '\"'.", 1, 9);
}

#[test]
fn reports_json_record_key_errors_with_utf16_positions() {
    assert_error("let x = {\"a\\q\":1}", "Bad escaped character in JSON at position 3 (line 1 column 4)", 1, 10);
    assert_error("let x = {\"😀\\u12z4\":1}", "Bad Unicode escape in JSON at position 7 (line 1 column 8)", 1, 10);
    assert_error(
        "let x = {\"a\tb\":1}",
        "Bad control character in string literal in JSON at position 2 (line 1 column 3)",
        1,
        10,
    );
}

#[test]
fn bounds_source_tokens_and_recursive_nesting() {
    assert!(CompiledScript::parse(&";".repeat(40_000)).is_ok());
    assert_error(&";".repeat(40_001), "Token limit exceeded.", 1, 40_001);
    assert_error(&format!("#{}", "😀".repeat(100_000)), "Script size limit is 200,000 characters.", 1, 1);
    assert!(CompiledScript::parse(&format!("{}1", "+".repeat(62))).is_ok());
    assert_error(&format!("{}1", "+".repeat(63)), "Expression nesting limit exceeded.", 1, 64);
    assert!(CompiledScript::parse(&format!("{}{}", "if 1 {".repeat(47), "}".repeat(47))).is_ok());
    assert_error(&format!("{}{}", "if 1 {".repeat(48), "}".repeat(48)), "Nesting limit exceeded.", 1, 289);
}
