// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! JSON-lines FORM evaluator for tools and reference-corpus harnesses.
//!
//! Each input line is one request, `{"source": "...", "overrides": {...}, "materials": [...]}`,
//! where `overrides` (default `{}`) and `materials` (default and `null`: no palette) are
//! optional. Scripts that `use` modules also name their own `path` and either `modules` (an
//! object mapping module paths to source text) or `root` (a folder modules are read from); paths
//! are relative to that root. Each request produces exactly one output line, in order:
//!
//! - `{"ok": true, "data": GeometryData, "evaluation_ms": number}` on success, where
//!   `evaluation_ms` is the parse-and-evaluate wall time in milliseconds; or
//! - `{"ok": false, "message": "...", "line": n, "column": n}` on any failure, with the FORM
//!   diagnostic location, or line 1, column 1 for transport and request errors.
//!
//! Requests are bounded at 32 MiB. An oversized line is rejected without buffering the rest of
//! it, and the next line is processed normally. The process exits when input ends or output
//! can no longer be written.

use std::io::{
    self,
    BufRead,
    Read,
    Write,
};
use std::path::PathBuf;
use std::time::Instant;

use joy_form::{
    CompiledScript,
    Diagnostic,
};
use serde_json::{
    Value,
    json,
};

/// Largest accepted request, excluding its line terminator.
const MAX_REQUEST_BYTES: usize = 32 * 1024 * 1024;
const OVERSIZED_REQUEST_MESSAGE: &str = "Request exceeds 32 MiB";

/// One framed input line.
#[derive(Debug, PartialEq)]
enum RequestLine {
    /// A complete request body without its newline.
    Request(Vec<u8>),
    /// A line longer than the request limit; its bytes were discarded.
    Oversized,
    /// Input ended.
    End,
}

fn main() {
    let mut input = io::stdin().lock();
    let mut output = io::BufWriter::new(io::stdout().lock());
    loop {
        let response = match read_request(&mut input, MAX_REQUEST_BYTES) {
            Ok(RequestLine::End) => break,
            Ok(RequestLine::Request(line)) => evaluate_request(&line),
            Ok(RequestLine::Oversized) => transport_failure(OVERSIZED_REQUEST_MESSAGE),
            Err(error) => {
                // A failed read leaves framing unknown; report it once and stop.
                let _ = write_response(&mut output, &transport_failure(&error.to_string()));
                break;
            }
        };
        if write_response(&mut output, &response).is_err() {
            break;
        }
    }
}

/// Read one newline-terminated request of at most `limit` bytes.
///
/// Reading is byte-oriented, so a UTF-8 sequence split by the limit cannot desynchronize framing.
/// An oversized line allocates at most `limit + 1` bytes; the remainder is skipped in place.
fn read_request(input: &mut impl BufRead, limit: usize) -> io::Result<RequestLine> {
    let mut line = Vec::new();
    let read = input.by_ref().take(limit as u64 + 1).read_until(b'\n', &mut line)?;
    if read == 0 {
        return Ok(RequestLine::End);
    }
    if line.last() == Some(&b'\n') {
        line.pop();
        return Ok(RequestLine::Request(line));
    }
    if line.len() > limit {
        input.skip_until(b'\n')?;
        return Ok(RequestLine::Oversized);
    }

    // The final request may end without a newline.
    Ok(RequestLine::Request(line))
}

/// Evaluate one request body and return its response object.
fn evaluate_request(line: &[u8]) -> Value {
    let request: Value = match serde_json::from_slice(line) {
        Ok(request) => request,
        Err(error) => return transport_failure(&error.to_string()),
    };
    let Some(source) = request["source"].as_str() else {
        return transport_failure("source must be text");
    };
    let overrides = request.get("overrides").cloned().unwrap_or_else(|| json!({}));
    let materials = request.get("materials").filter(|materials| !materials.is_null());

    let started = Instant::now();
    let result = parse(&request, source).and_then(|script| script.evaluate(&overrides, materials));
    let evaluation_ms = started.elapsed().as_secs_f64() * 1000.0;
    match result {
        Ok(data) => json!({"ok": true, "data": data, "evaluation_ms": evaluation_ms}),
        Err(diagnostic) => failure(diagnostic),
    }
}

/// Parse the request's script, loading modules from its `modules` map or its `root` folder when it names a `path`.
fn parse(request: &Value, source: &str) -> Result<CompiledScript, Diagnostic> {
    let Some(path) = request.get("path").and_then(Value::as_str) else {
        return CompiledScript::parse(source);
    };
    if let Some(modules) = request.get("modules").and_then(Value::as_object) {
        let loader = |module: &str| -> Result<String, String> {
            modules
                .get(module)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| "not in the request's modules".to_owned())
        };
        return CompiledScript::parse_with_modules(source, path, &loader);
    }
    let root = PathBuf::from(request.get("root").and_then(Value::as_str).unwrap_or("."));
    let loader = |module: &str| -> Result<String, String> {
        std::fs::read_to_string(root.join(module)).map_err(|error| error.to_string())
    };
    CompiledScript::parse_with_modules(source, path, &loader)
}

fn failure(diagnostic: Diagnostic) -> Value {
    json!({"ok": false, "message": diagnostic.message, "line": diagnostic.line, "column": diagnostic.column})
}

/// Report a request that never reached FORM, at the protocol's unlocated position.
fn transport_failure(message: &str) -> Value {
    failure(Diagnostic {
        message: message.to_owned(),
        line: 1,
        column: 1,
    })
}

fn write_response(output: &mut impl Write, response: &Value) -> io::Result<()> {
    writeln!(output, "{response}")?;
    output.flush()
}
