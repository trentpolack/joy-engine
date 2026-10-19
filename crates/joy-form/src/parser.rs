// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Tokenizer and recursive-descent parser for FORM source.

use crate::Diagnostic;
use crate::ast::{
    ExprKind,
    Expression,
    Statement,
    StmtKind,
    Token,
};
use std::collections::HashSet;

const SOURCE_LIMIT: usize = 200_000;
const TOKEN_LIMIT: usize = 40_000;
const BLOCK_DEPTH_LIMIT: usize = 48;
const EXPRESSION_DEPTH_LIMIT: usize = 64;
const INVALID_DATA_KEY: &str = "Invalid user data field name (1–64 characters; prototype keys are reserved).";

/// User data field names FORM rejects. Exported user data lands in glTF `extras`, and these names would alter object
/// prototypes in the JavaScript viewers and pipelines that read those files.
pub(crate) const RESERVED_DATA_KEYS: [&str; 3] = ["__proto__", "prototype", "constructor"];

/// The byte-order mark some editors write at the start of a text file.
const BYTE_ORDER_MARK: char = '\u{feff}';

/// Parse FORM without executing authored code. Locations are one-based lines and
/// character columns, so editors place diagnostics directly.
pub(crate) fn parse(source: &str) -> Result<Vec<Statement>, Diagnostic> {
    Parser { tokens: tokenize(source)?, cursor: 0, depth: 0 }.block(false)
}

struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
    // Blocks and expressions deliberately share the original parser's budget.
    depth: usize,
}

impl Parser {
    fn current(&self) -> &Token {
        &self.tokens[self.cursor]
    }

    fn take(&mut self, text: &str) -> bool {
        if self.current().text != text {
            return false;
        }
        self.cursor += 1;
        true
    }

    fn expect(&mut self, text: &str) -> Result<(), Diagnostic> {
        if self.take(text) {
            return Ok(());
        }
        Err(Diagnostic::at(format!("Expected '{text}', found '{}'.", self.current().text), self.current()))
    }

    fn name(&mut self) -> Result<String, Diagnostic> {
        if !is_name(&self.current().text) {
            return Err(Diagnostic::at("Expected a variable name.", self.current()));
        }
        let name = self.current().text.clone();
        self.cursor += 1;
        Ok(name)
    }

    fn block(&mut self, nested: bool) -> Result<Vec<Statement>, Diagnostic> {
        self.depth += 1;
        if self.depth > BLOCK_DEPTH_LIMIT {
            return Err(Diagnostic::at("Nesting limit exceeded.", self.current()));
        }
        let mut statements = Vec::new();
        while self.current().text != "<end>" && self.current().text != "}" {
            if self.take(";") {
                continue;
            }
            statements.push(self.statement()?);
        }
        if nested {
            self.expect("}")?;
        } else if self.current().text == "}" {
            return Err(Diagnostic::at("Unexpected '}'.", self.current()));
        }
        self.depth -= 1;
        Ok(statements)
    }

    fn statement(&mut self) -> Result<Statement, Diagnostic> {
        let token = self.current().clone();
        let kind = if self.take("project") {
            return self.project(token);
        } else if self.take("with") {
            self.expect("userData")?;
            self.expect("(")?;
            let expression = self.expression(0)?;
            self.expect(")")?;
            self.expect("{")?;
            StmtKind::WithUserData(expression, self.block(true)?)
        } else if self.take("wrangle") {
            let domain = self.name()?;
            self.expect("in")?;
            let expression = self.expression(0)?;
            self.expect("as")?;
            let binding = self.name()?;
            self.expect("{")?;
            StmtKind::Wrangle { domain, expression, binding: Some(binding), body: self.block(true)? }
        } else if self.take("if") {
            let expression = self.expression(0)?;
            self.expect("{")?;
            StmtKind::If(expression, self.block(true)?)
        } else if self.take("let") {
            let name = self.name()?;
            self.expect("=")?;
            StmtKind::Let(name, self.expression(0)?)
        } else if self.take("param") {
            return self.parameter(token);
        } else if self.take("repeat") {
            let expression = self.expression(0)?;
            self.expect("as")?;
            let name = self.name()?;
            self.expect("{")?;
            StmtKind::Repeat { name, expression, body: self.block(true)? }
        } else if self.tokens.get(self.cursor + 1).map(|next| next.text.as_str()) == Some(".")
            && self.tokens.get(self.cursor + 3).map(|next| next.text.as_str()) == Some("=")
        {
            let binding = self.name()?;
            self.expect(".")?;
            let name = self.name()?;
            self.expect("=")?;
            StmtKind::BindingAttribute { binding, name, expression: self.expression(0)? }
        } else {
            StmtKind::Command(self.expression(0)?)
        };
        Ok(Statement { kind, token })
    }

    fn parameter(&mut self, token: Token) -> Result<Statement, Diagnostic> {
        let name = self.name()?;
        self.expect("=")?;
        let mut values = vec![self.expression(0)?];
        let mut metadata: Vec<(String, Expression)> = Vec::new();
        if self.take(",") {
            // The legacy form always has exactly four positional expressions.
            values.push(self.expression(0)?);
            for _ in 0..2 {
                self.expect(",")?;
                values.push(self.expression(0)?);
            }
        } else if self.take("meta") {
            self.expect("(")?;
            if !self.take(")") {
                loop {
                    let key = self.name()?;
                    if !matches!(key.as_str(), "min" | "max" | "step") {
                        return Err(Diagnostic::at(format!("Unknown parameter metadata '{key}'."), self.current()));
                    }
                    if metadata.iter().any(|(existing, _)| existing == &key) {
                        return Err(Diagnostic::at(format!("Duplicate metadata '{key}'."), self.current()));
                    }
                    self.expect("=")?;
                    metadata.push((key, self.expression(0)?));
                    if !self.take(",") {
                        break;
                    }
                }
                self.expect(")")?;
            }
        }
        Ok(Statement { kind: StmtKind::Param { name, values, metadata }, token })
    }

    fn project(&mut self, token: Token) -> Result<Statement, Diagnostic> {
        self.expect("meta")?;
        self.expect("(")?;
        let mut fields = serde_json::Map::new();
        let mut name_token = None;
        loop {
            let key_token = self.current().clone();
            let key = self.name()?;
            if !matches!(key.as_str(), "name" | "info" | "title") {
                return Err(Diagnostic::at(format!("Unknown project metadata '{key}'."), &key_token));
            }
            if fields.contains_key(&key) {
                return Err(Diagnostic::at(format!("Duplicate project metadata '{key}'."), &key_token));
            }
            self.expect("=")?;
            let value_token = self.current();
            let value: String = serde_json::from_str(&value_token.text)
                .map_err(|_| Diagnostic::at("Project metadata requires double-quoted text.", value_token))?;
            let text_limit = if key == "info" { 160 } else { 120 };
            if value.chars().count() > text_limit {
                return Err(Diagnostic::at("Project metadata text is too long.", value_token));
            }
            if key == "name" && (value.trim().is_empty() || value.contains(['\r', '\n'])) {
                return Err(Diagnostic::at("Project name requires nonempty, single-line text.", value_token));
            }
            if key == "name" {
                name_token = Some(value_token.clone());
            }
            fields.insert(key, serde_json::Value::String(value));
            self.cursor += 1;
            if !self.take(",") {
                break;
            }
        }
        self.expect(")")?;
        let Some(name) = name_token else {
            return Err(Diagnostic::at("Project metadata requires a name.", &token));
        };
        Ok(Statement { kind: StmtKind::Project { fields: serde_json::Value::Object(fields), name }, token })
    }

    fn expression(&mut self, minimum: i32) -> Result<Expression, Diagnostic> {
        self.depth += 1;
        if self.depth > EXPRESSION_DEPTH_LIMIT {
            return Err(Diagnostic::at("Expression nesting limit exceeded.", self.current()));
        }
        let token = self.current().clone();
        self.cursor += 1;
        let mut left = match token.text.as_str() {
            "{" => Expression { kind: ExprKind::Record(self.record_fields()?), token: token.clone() },
            "[" => Expression { kind: ExprKind::Array(self.arguments("]")?), token: token.clone() },
            "true" | "false" | "null" => Expression {
                kind: ExprKind::Literal(match token.text.as_str() {
                    "true" => serde_json::Value::Bool(true),
                    "false" => serde_json::Value::Bool(false),
                    _ => serde_json::Value::Null,
                }),
                token: token.clone(),
            },
            "-" | "+" => Expression {
                kind: ExprKind::Unary(token.text.clone(), Box::new(self.expression(30)?)),
                token: token.clone(),
            },
            "(" => {
                let expression = self.expression(0)?;
                self.expect(")")?;
                expression
            }
            text if text.starts_with('"') => {
                let text = serde_json::from_str(text).map_err(|_| Diagnostic::at("Invalid quoted text.", &token))?;
                Expression { kind: ExprKind::Text(text), token: token.clone() }
            }
            text if starts_number(text.as_bytes()) => Expression {
                // Tokenization already enforces the decimal/exponent grammar.
                kind: ExprKind::Number(text.parse().unwrap_or(f64::NAN)),
                token: token.clone(),
            },
            text if is_name(text) => {
                let kind = if self.take("(") {
                    ExprKind::Call(text.to_owned(), self.arguments(")")?)
                } else {
                    ExprKind::Variable(text.to_owned())
                };
                Expression { kind, token: token.clone() }
            }
            _ => {
                return Err(Diagnostic::at(format!("Expected an expression, found '{}'.", token.text), &token));
            }
        };
        while self.take(".") {
            left = Expression {
                kind: ExprKind::Component(Box::new(left), self.name()?),
                // A component uses the start token, including an opening parenthesis.
                token: token.clone(),
            };
        }
        while precedence(&self.current().text) >= minimum {
            let operator = self.current().clone();
            self.cursor += 1;
            let next_minimum = precedence(&operator.text) + if operator.text == "^" { 0 } else { 1 };
            let right = self.expression(next_minimum)?;
            left = Expression {
                kind: ExprKind::Binary(operator.text.clone(), Box::new(left), Box::new(right)),
                token: operator,
            };
        }
        self.depth -= 1;
        Ok(left)
    }

    fn arguments(&mut self, closing: &str) -> Result<Vec<Expression>, Diagnostic> {
        let mut arguments = Vec::new();
        if !self.take(closing) {
            loop {
                arguments.push(self.expression(0)?);
                if !self.take(",") {
                    break;
                }
            }
            self.expect(closing)?;
        }
        Ok(arguments)
    }

    fn record_fields(&mut self) -> Result<Vec<(String, Expression)>, Diagnostic> {
        let mut fields: Vec<(String, Expression)> = Vec::new();
        let mut names = HashSet::new();
        if self.take("}") {
            return Ok(fields);
        }
        loop {
            let key_token = self.current().clone();
            let key: String = if key_token.text.starts_with('"') {
                let key = serde_json::from_str(&key_token.text)
                    .map_err(|error| Diagnostic::at(quoted_key_error(&key_token.text, error), &key_token))?;
                self.cursor += 1;
                key
            } else {
                self.name()?
            };
            if key.is_empty() || key.chars().count() > 64 || RESERVED_DATA_KEYS.contains(&key.as_str()) {
                return Err(Diagnostic::at(INVALID_DATA_KEY, &key_token));
            }
            if !names.insert(key.clone()) {
                return Err(Diagnostic::at(format!("Duplicate user data field '{key}'."), &key_token));
            }
            self.expect(":")?;
            fields.push((key, self.expression(0)?));
            if !self.take(",") {
                break;
            }
        }
        self.expect("}")?;
        // Fields evaluate, and are stored, in authored order.
        Ok(fields)
    }
}

/// Describe a malformed quoted record key, naming the offending character's one-based
/// position within the key token. Ordinary string expressions have their own diagnostic.
fn quoted_key_error(text: &str, error: serde_json::Error) -> String {
    let diagnostic = |description: &str, byte_offset: usize| {
        let position = text[..byte_offset].chars().count() + 1;
        format!("{description} in quoted key at character {position}.")
    };
    let mut characters = text.char_indices().skip(1);
    while let Some((offset, character)) = characters.next() {
        if character <= '\u{001f}' {
            return diagnostic("Bad control character in string literal", offset);
        }
        if character != '\\' {
            continue;
        }
        let Some((escape_offset, escape)) = characters.next() else {
            break;
        };
        match escape {
            '"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' => {}
            'u' => {
                for _ in 0..4 {
                    let Some((digit_offset, digit)) = characters.next() else {
                        return diagnostic("Bad Unicode escape", text.len());
                    };
                    if !digit.is_ascii_hexdigit() {
                        return diagnostic("Bad Unicode escape", digit_offset);
                    }
                }
            }
            _ => return diagnostic("Bad escaped character", escape_offset),
        }
    }
    // A `\u` escape naming a lone surrogate has no character to stand for. Reject it
    // explicitly instead of silently replacing authored data with U+FFFD.
    format!("Invalid quoted key: {error}.")
}

fn precedence(operator: &str) -> i32 {
    match operator {
        "==" | "!=" | "<" | ">" | "<=" | ">=" => 5,
        "+" | "-" => 10,
        "*" | "/" | "%" => 20,
        "^" => 40,
        _ => -1,
    }
}

fn is_name(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.first().is_some_and(|byte| is_name_start(*byte)) && bytes.iter().all(|byte| is_name_continue(*byte))
}

fn is_name_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_name_continue(byte: u8) -> bool {
    is_name_start(byte) || byte.is_ascii_digit()
}

fn starts_number(bytes: &[u8]) -> bool {
    bytes.first().is_some_and(u8::is_ascii_digit)
        || (bytes.first() == Some(&b'.') && bytes.get(1).is_some_and(u8::is_ascii_digit))
}

fn tokenize(source: &str) -> Result<Vec<Token>, Diagnostic> {
    if source.chars().count() > SOURCE_LIMIT {
        return Err(Diagnostic { message: "Script size limit is 200,000 characters.".into(), line: 1, column: 1 });
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
            return Err(Diagnostic { message: format!("Unexpected character '{character}'."), line, column });
        };
        let text = &remaining[..length];
        tokens.push(Token { text: text.to_owned(), line, column });
        if tokens.len() > TOKEN_LIMIT {
            return Err(Diagnostic::at("Token limit exceeded.", tokens.last().unwrap()));
        }
        offset += length;
        column += text.chars().count();
    }
    tokens.push(Token { text: "<end>".into(), line, column });
    Ok(tokens)
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
    if ["==", "!=", "<=", ">="].iter().any(|operator| source.starts_with(operator)) {
        return Some(2);
    }
    if b"<>+-*/%^=(),{};.[]:".contains(&bytes[0]) {
        return Some(1);
    }
    None
}
