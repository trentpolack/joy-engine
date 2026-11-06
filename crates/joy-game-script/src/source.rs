// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Reject platform power operations while accepting ordinary Lua strings and comments.

/// Lua's `^` invokes platform `pow`; gameplay must call portable `powf` instead. Quoted strings, long strings,
/// line comments, and long comments contain no executable operators.
pub(crate) fn validate(source: &str) -> mlua::Result<()> {
    let bytes = source.as_bytes();
    let mut position = 0;
    while position < bytes.len() {
        if bytes[position] == b'-' && bytes.get(position + 1) == Some(&b'-') {
            position += 2;
            if let Some(end) = long_string_end(bytes, position) {
                position = end;
            } else {
                while position < bytes.len() && !matches!(bytes[position], b'\r' | b'\n') {
                    position += 1;
                }
            }
        } else if bytes[position] == b'\'' || bytes[position] == b'"' {
            let quote = bytes[position];
            position += 1;
            while position < bytes.len() {
                if bytes[position] == b'\\' {
                    position += 2;
                } else if bytes[position] == quote {
                    position += 1;
                    break;
                } else {
                    position += 1;
                }
            }
        } else if let Some(end) = long_string_end(bytes, position) {
            position = end;
        } else {
            if bytes[position] == b'^' {
                return Err(mlua::Error::runtime(
                    "the platform power operator is disabled; use portable powf(base, exponent)",
                ));
            }
            position += 1;
        }
    }
    Ok(())
}

fn long_string_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start) != Some(&b'[') {
        return None;
    }
    let mut opening = start + 1;
    while bytes.get(opening) == Some(&b'=') {
        opening += 1;
    }
    if bytes.get(opening) != Some(&b'[') {
        return None;
    }
    let equals = opening - start - 1;
    let mut position = opening + 1;
    while position < bytes.len() {
        if bytes[position] == b']' {
            let closing = position + equals + 1;
            if bytes.get(closing) == Some(&b']') && bytes[position + 1..closing].iter().all(|byte| *byte == b'=') {
                return Some(closing + 1);
            }
        }
        position += 1;
    }
    Some(bytes.len())
}
