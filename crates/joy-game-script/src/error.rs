// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Typed failures for compiling and calling game scripts. Messages are stable and lead with the script path and the
//! one-based line and column, so hosts can show them verbatim in logs and development overlays.

use std::fmt;

use thiserror::Error;

/// Where in a script a failure happened. Line and column are one-based; zero means the position is unknown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptLocation {
    /// The host's path for the script, as passed to [`crate::Script::compile`].
    pub path: String,
    /// One-based line, or zero.
    pub line: usize,
    /// One-based column in characters, or zero.
    pub column: usize,
}

impl fmt::Display for ScriptLocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            return write!(formatter, "{}", self.path);
        }
        write!(formatter, "{}:{}:{}", self.path, self.line, self.column)
    }
}

/// A script failed to compile, lacked a hook its host calls, or failed while running.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ScriptError {
    /// The source did not parse, or its top level failed while defining constants.
    #[error("{location}: {message}")]
    Compile {
        /// Where parsing stopped.
        location: ScriptLocation,
        /// The language's description of the problem.
        message: String,
    },

    /// The script does not define a hook its host requires.
    #[error("{path}: missing hook `function {name}` with {arity} parameter(s)")]
    MissingHook {
        /// The script's path.
        path: String,
        /// The hook's function name.
        name: String,
        /// The parameter count the host calls it with (`this` is not counted).
        arity: usize,
    },

    /// The host called a hook it did not declare when compiling the script.
    #[error("{path}: `{name}` is not a declared hook of this script")]
    UndeclaredHook {
        /// The script's path.
        path: String,
        /// The function name the host asked for.
        name: String,
    },

    /// A hook call failed: a runtime error, a host function's error, or an exceeded [`crate::ScriptLimits`] limit.
    #[error("{location}: {message} (in {call_stack})")]
    Runtime {
        /// Where the innermost failure happened.
        location: ScriptLocation,
        /// The language's description of the problem.
        message: String,
        /// The hook and the script functions it was inside, outermost first (`think → move_frame`).
        call_stack: String,
    },

    /// A host value could not become a script value, or a script value did not match the host type it was read as.
    #[error("{context}: {message}")]
    Conversion {
        /// What was being converted (a script path and constant, or a host type).
        context: String,
        /// The serializer's description of the mismatch, naming the field.
        message: String,
    },

    /// A source the library was asked to compile or import does not exist.
    #[error("{path}: no script named `{name}` (require names sources added to the library, without `.lua`)")]
    UnknownScript {
        /// The script that asked for it, or the name itself when the host asked.
        path: String,
        /// The missing name.
        name: String,
    },

    /// A hook returned a value of the wrong type.
    #[error("{path}: hook `{hook}` returned {found}, expected {expected}")]
    ReturnType {
        /// The script's path.
        path: String,
        /// The hook's function name.
        hook: String,
        /// The type the host reads.
        expected: String,
        /// The type the hook returned.
        found: String,
    },
}

impl ScriptError {
    pub(crate) fn from_compile(path: &str, error: mlua::Error) -> Self {
        let message = error.to_string();
        Self::Compile {
            location: location(path, &message),
            message,
        }
    }

    pub(crate) fn from_runtime(path: &str, hook: &str, error: mlua::Error) -> Self {
        let message = error.to_string();
        let call_stack = match message.split_once("stack traceback:") {
            Some((_, stack)) => format!("{hook} → {}", stack.trim()),
            None => hook.to_string(),
        };
        Self::Runtime {
            location: location(path, &message),
            message,
            call_stack,
        }
    }
}

/// Lua reports line numbers but no stable column. Keep its complete traceback in the error message.
fn location(fallback: &str, message: &str) -> ScriptLocation {
    for line in message.lines() {
        let pieces: Vec<_> = line.trim().split(':').collect();
        for (index, piece) in pieces.iter().enumerate().skip(1) {
            if let Ok(number) = piece.parse::<usize>() {
                let path = pieces[index - 1].trim().trim_start_matches("[string \"").trim_end_matches("\"]");
                return ScriptLocation {
                    path: path.to_string(),
                    line: number,
                    column: 0,
                };
            }
        }
    }
    ScriptLocation {
        path: fallback.into(),
        line: 0,
        column: 0,
    }
}
