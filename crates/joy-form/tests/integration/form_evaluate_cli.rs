// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Process-level checks for the `joy-form-evaluate` JSON-lines protocol: request framing at the 32 MiB limit,
//! recovery after an oversized request, and a final request that ends without a newline.

use std::io::Write;
use std::process::{
    Command,
    Stdio,
};

use serde_json::{
    Value,
    json,
};

/// The evaluator's request limit; the newline that ends a request does not count toward it.
const REQUEST_LIMIT_BYTES: usize = 32 * 1024 * 1024;

/// A small request whose successful response is easy to check.
const POINT_REQUEST: &[u8] = br#"{"source":"let g=geometry() addPoint(g,vector(1,2,3)) output(\"P\",g)"}"#;

#[test]
fn oversized_unicode_request_is_rejected_once_and_next_request_survives() {
    // Put a two-byte UTF-8 character across the 32 MiB edge. Rejection must drain bytes rather
    // than lose line framing because that character cannot be decoded in isolation.
    let mut input = vec![b' '; REQUEST_LIMIT_BYTES];
    input.extend_from_slice("é\n".as_bytes());
    input.extend_from_slice(POINT_REQUEST);
    input.push(b'\n');
    input.extend_from_slice(b"{\"source\":\"\\nlet a = missing\"}\n");

    let lines = evaluate(&input);
    assert_eq!(lines.len(), 3);
    assert_eq!((&lines[0]["ok"], &lines[0]["message"]), (&json!(false), &json!("Request exceeds 32 MiB")));
    assert_point_response(&lines[1]);
    assert_eq!((&lines[2]["ok"], &lines[2]["line"], &lines[2]["column"]), (&json!(false), &json!(2), &json!(9)));
}

#[test]
fn request_of_exactly_the_size_limit_is_accepted() {
    // Leading spaces pad the request to the limit; JSON allows white space before a value.
    let mut input = vec![b' '; REQUEST_LIMIT_BYTES - POINT_REQUEST.len()];
    input.extend_from_slice(POINT_REQUEST);
    input.push(b'\n');

    let lines = evaluate(&input);
    assert_eq!(lines.len(), 1);
    assert_point_response(&lines[0]);
}

#[test]
fn final_request_without_a_trailing_newline_is_answered() {
    // Input that ends mid-line, as from `printf '...' | joy-form-evaluate`, is still one request.
    let lines = evaluate(POINT_REQUEST);
    assert_eq!(lines.len(), 1);
    assert_point_response(&lines[0]);
}

#[test]
fn requests_load_modules_from_their_own_map() {
    let request = json!({
        "source": "use \"parts/kit.form\" as kit\noutput(\"Slab\", kit.slab(2))",
        "path": "city/block.form",
        "modules": {"city/parts/kit.form": "fn slab(width) { return box(width, 1, width) }"},
    });
    let lines = evaluate(format!("{request}\n").as_bytes());
    assert_eq!((&lines[0]["ok"], lines[0]["data"]["positions"].as_array().map(Vec::len)), (&json!(true), Some(24)));
}

/// Run the evaluator over `input`, closing its input afterward, and return its response lines.
///
/// All input is written before any output is read. That is safe because each response is one short line, far below
/// what the output pipe buffers.
fn evaluate(input: &[u8]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_joy-form-evaluate"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the evaluator binary starts");
    child.stdin.take().expect("piped stdin").write_all(input).unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

/// Check that `response` is the successful result of `POINT_REQUEST`.
fn assert_point_response(response: &Value) {
    assert_eq!((&response["ok"], &response["data"]["positions"]), (&json!(true), &json!([1.0, 2.0, 3.0])));
}
