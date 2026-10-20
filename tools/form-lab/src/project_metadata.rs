// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Renaming a document through its script's `project meta(name="…")` declaration.

use joy_engine::form::{
    CompiledScript,
    project_name_tokens,
};
use joy_engine::serde_json;

/// A replacement of the byte range `from..to` of the script with `insert`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextChange {
    /// Start byte offset.
    pub from: usize,
    /// End byte offset (exclusive).
    pub to: usize,
    /// Replacement text.
    pub insert: String,
}

impl TextChange {
    /// Apply the change to `source`, which must be the text the change was computed for.
    pub fn apply(&self, source: &str) -> String {
        format!("{}{}{}", &source[..self.from], self.insert, &source[self.to..])
    }
}

/// Return a minimal edit that makes the script own the document name.
///
/// Existing metadata formatting and other fields remain untouched; scripts without metadata get a declaration on their
/// first line. Invalid syntax, an invalid name, or more than one declaration fails, so the source is never guessed at.
pub fn project_name_change(source: &str, name: &str) -> Result<TextChange, String> {
    let quoted = serde_json::to_string(name).map_err(|error| error.to_string())?;
    let declaration = format!("project meta(name={quoted})\n");
    CompiledScript::parse(&declaration).map_err(|diagnostic| diagnostic.message)?;
    let tokens = project_name_tokens(source).map_err(|diagnostic| diagnostic.message)?;
    if tokens.len() > 1 {
        return Err("Declare project metadata only once before renaming.".into());
    }
    let Some(token) = tokens.first() else {
        return Ok(TextChange {
            from: 0,
            to: 0,
            insert: declaration,
        });
    };
    let from = byte_offset(source, token.line, token.column).ok_or("Project metadata is outside the script.")?;
    Ok(TextChange {
        from,
        to: from + token.text.len(),
        insert: quoted,
    })
}

/// Byte offset of a one-based line and one-based character column, or `None` beyond the text.
pub fn byte_offset(source: &str, line: usize, column: usize) -> Option<usize> {
    let mut start = 0;
    for _ in 1..line {
        start += source[start..].find('\n')? + 1;
    }
    for (characters, (offset, character)) in source[start..].char_indices().enumerate() {
        if characters >= column.saturating_sub(1) || character == '\n' {
            return Some(start + offset);
        }
    }
    Some(source.len())
}
