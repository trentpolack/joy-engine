// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Process-level checks for the `joy-form-evaluate` JSON-lines protocol.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

#[test]
fn oversized_unicode_request_is_rejected_once_and_next_request_survives() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_joy-form-evaluate"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the evaluator binary starts");
    let mut input = child.stdin.take().expect("piped stdin");

    // Put a two-byte UTF-8 character across the 32 MiB edge. Rejection must drain bytes rather
    // than lose line framing because that character cannot be decoded in isolation.
    input.write_all(&vec![b' '; 32 * 1024 * 1024]).unwrap();
    input.write_all("é\n".as_bytes()).unwrap();
    input.write_all(b"{\"source\":\"point(1,2,3)\"}\n").unwrap();
    input.write_all(b"{\"source\":\"\\npoint(missing,0,0)\"}\n").unwrap();
    drop(input);

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let lines: Vec<Value> =
        String::from_utf8(output.stdout).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(lines.len(), 3);
    assert_eq!((&lines[0]["ok"], &lines[0]["message"]), (&json!(false), &json!("Request exceeds 32 MiB")));
    assert_eq!((&lines[1]["ok"], &lines[1]["data"]["positions"]), (&json!(true), &json!([1.0, 2.0, 3.0])));
    assert_eq!((&lines[2]["ok"], &lines[2]["line"], &lines[2]["column"]), (&json!(false), &json!(2), &json!(7)));
}
