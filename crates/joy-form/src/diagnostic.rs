// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The located error every FORM failure reports.

use serde::Serialize;

use crate::ast::Token;

/// A parse or evaluation failure with its source location.
///
/// `line` is one-based. `column` is one-based and counts characters, as FORM EDITOR's script editor
/// does. Failures without a source position (overrides, palettes, primitive helpers, and
/// transport errors) report line 1, column 1.
#[derive(Serialize, Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message} (line {line}, column {column})")]
pub struct Diagnostic {
    /// Human-readable description of the failure.
    pub message: String,
    /// One-based source line.
    pub line: usize,
    /// One-based source column in characters.
    pub column: usize,
}

impl Diagnostic {
    /// Create a diagnostic without a meaningful source position.
    pub(crate) fn unlocated(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            line: 1,
            column: 1,
        }
    }

    /// Create a diagnostic at a token's start.
    pub(crate) fn at(message: impl Into<String>, token: &Token) -> Self {
        Self {
            message: message.into(),
            line: token.line,
            column: token.column,
        }
    }
}

/// A rejected FORM resource or document value (material palette, parameter override, or mesh), with its stable
/// authored diagnostic message. Values have no source position, so this carries no line or column.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct FormValueError(pub String);

impl From<String> for FormValueError {
    fn from(message: String) -> Self {
        Self(message)
    }
}

/// A located token from FORM source, such as the quoted name of a `project meta(name="…")` declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceToken {
    /// The token's source text, including quotes for text literals.
    pub text: String,
    /// One-based source line.
    pub line: usize,
    /// One-based source column in characters.
    pub column: usize,
}
