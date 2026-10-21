// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM source tokenization: names, numbers, quoted text, and operators with one-based line and character columns.

use crate::{
    Diagnostic,
    ast::Token,
};

/// Largest accepted source, in characters.
pub(crate) const SOURCE_LIMIT: usize = 200_000;

/// Largest accepted token count per source file.
pub(crate) const TOKEN_LIMIT: usize = 40_000;

/// Text of the sentinel token that ends every token stream.
pub(crate) const END_TOKEN: &str = "<end>";

/// The byte-order mark some editors write at the start of a text file.
const BYTE_ORDER_MARK: char = '\u{feff}';

/// Two-character operators, matched before their one-character prefixes.
const TWO_CHARACTER_OPERATORS: [&str; 8] = ["==", "!=", "<=", ">=", "+=", "-=", "*=", "/="];

/// One-character operators and punctuation.
const ONE_CHARACTER_OPERATORS: &[u8] = b"<>+-*/%^=(),{};.[]:";

/// Split `source` into tokens followed by an [`END_TOKEN`]. Comments (`#` or `//` to the end of the line) and
/// whitespace separate tokens and are dropped.
pub(crate) fn tokenize(source: &str) -> Result<Vec<Token>, Diagnostic> {
    if source.chars().count() > SOURCE_LIMIT {
        return Err(Diagnostic {
            message: "Script size limit is 200,000 characters.".into(),
            line: 1,
            column: 1,
        });
    }

    // A leading byte-order mark is an encoding artifact, not source; it takes no column.
    let source = source.strip_prefix(BYTE_ORDER_MARK).unwrap_or(source);
    let mut tokens = Vec::new();
    let mut offset = 0;
    let mut line = 1;
    let mut column = 1;
    while offset < source.len() {
        let remaining = &source[offset..];
        let character = remaining.chars().next().unwrap();
        if character == '#' || remaining.starts_with("//") {
            let length = remaining.find('\n').unwrap_or(remaining.len());
            column += remaining[..length].chars().count();
            offset += length;
            continue;
        }
        if character.is_whitespace() {
            if character == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
            offset += character.len_utf8();
            continue;
        }
        if character == '@' {
            return Err(Diagnostic {
                message: "@attribute syntax is no longer supported. Use 'wrangle points in geometry as point' and 'point.name'; declare fields first with addAttribute().".into(),
                line,
                column,
            });
        }
        let Some(length) = token_length(remaining) else {
            return Err(Diagnostic {
                message: format!("Unexpected character '{character}'."),
                line,
                column,
            });
        };
        let text = &remaining[..length];
        tokens.push(Token {
            text: text.to_owned(),
            line,
            column,
        });
        if tokens.len() > TOKEN_LIMIT {
            return Err(Diagnostic::at("Token limit exceeded.", tokens.last().unwrap()));
        }
        offset += length;
        column += text.chars().count();
    }
    tokens.push(Token {
        text: END_TOKEN.into(),
        line,
        column,
    });
    Ok(tokens)
}

/// Whether `text` is an identifier: a letter or underscore followed by letters, digits, or underscores.
pub(crate) fn is_name(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.first().is_some_and(|byte| is_name_start(*byte)) && bytes.iter().all(|byte| is_name_continue(*byte))
}

/// Whether `text` begins a number literal (a digit, or a decimal point followed by a digit).
pub(crate) fn starts_number(bytes: &[u8]) -> bool {
    bytes.first().is_some_and(u8::is_ascii_digit)
        || (bytes.first() == Some(&b'.') && bytes.get(1).is_some_and(u8::is_ascii_digit))
}

fn is_name_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_name_continue(byte: u8) -> bool {
    is_name_start(byte) || byte.is_ascii_digit()
}

/// Measure the token at the start of `source` without a regex engine. An incomplete
/// exponent is left for the following tokens (`1e+` starts with the number token `1`).
fn token_length(source: &str) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes[0] == b'"' {
        let mut characters = source.char_indices().skip(1);
        while let Some((offset, character)) = characters.next() {
            match character {
                '"' => return Some(offset + 1),
                '\r' | '\n' => return None,
                '\\' => match characters.next() {
                    None | Some((_, '\r' | '\n' | '\u{2028}' | '\u{2029}')) => return None,
                    _ => {}
                },
                _ => {}
            }
        }
        return None;
    }
    if starts_number(bytes) {
        let mut length = 0;
        while bytes.get(length).is_some_and(u8::is_ascii_digit) {
            length += 1;
        }
        if bytes.get(length) == Some(&b'.') {
            length += 1;
            while bytes.get(length).is_some_and(u8::is_ascii_digit) {
                length += 1;
            }
        }
        if matches!(bytes.get(length), Some(b'e' | b'E')) {
            let mut exponent = length + 1;
            if matches!(bytes.get(exponent), Some(b'+' | b'-')) {
                exponent += 1;
            }
            let digits_start = exponent;
            while bytes.get(exponent).is_some_and(u8::is_ascii_digit) {
                exponent += 1;
            }
            if exponent > digits_start {
                length = exponent;
            }
        }
        return Some(length);
    }
    if is_name_start(bytes[0]) {
        return Some(bytes.iter().take_while(|byte| is_name_continue(**byte)).count());
    }
    if TWO_CHARACTER_OPERATORS.iter().any(|operator| source.starts_with(operator)) {
        return Some(2);
    }
    if ONE_CHARACTER_OPERATORS.contains(&bytes[0]) {
        return Some(1);
    }
    None
}
