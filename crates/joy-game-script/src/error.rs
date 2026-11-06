// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Typed failures for compiling and calling game scripts. Messages are stable and lead with the script path and the
//! one-based line and column, so hosts can show them verbatim in logs and development overlays.

use std::fmt;

use rhai::EvalAltResult;
use rhai::ParseError;
use rhai::Position;
use thiserror::Error;

use crate::script::DISPATCH_PREFIX;

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

impl ScriptLocation {
    /// A location in `path` at a Rhai position.
    pub(crate) fn at(path: &str, position: Position) -> Self {
        Self {
            path: path.to_string(),
            line: position.line().unwrap_or(0),
            column: position.position().unwrap_or(0),
        }
    }
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
    #[error("{path}: missing hook `fn {name}` with {arity} parameter(s)")]
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
    #[error("{path}: no script named `{name}` (imports name scripts added to the library, without `.rhai`)")]
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
    /// A parse failure in `path`.
    pub(crate) fn from_parse(path: &str, error: ParseError) -> Self {
        Self::Compile {
            location: ScriptLocation::at(path, error.1),
            message: error.0.to_string(),
        }
    }

    /// A runtime failure in `path` while running `hook`, unwrapping nested script calls and imports down to the
    /// innermost error so the location names the line (and the imported script) that failed rather than the line that
    /// called it.
    pub(crate) fn from_runtime(path: &str, hook: &str, operation_budget: u64, error: EvalAltResult) -> Self {
        let mut call_stack = vec![hook.to_string()];
        let mut location_path = path.to_string();
        let mut error = error;
        loop {
            match error {
                EvalAltResult::ErrorInFunctionCall(name, source, inner, _) => {
                    if !name.starts_with(DISPATCH_PREFIX) && call_stack.last() != Some(&name) {
                        call_stack.push(name);
                    }
                    if !source.is_empty() {
                        location_path = source;
                    }
                    error = *inner;
                }
                EvalAltResult::ErrorInModule(module, inner, _) => {
                    call_stack.push(format!("import \"{module}\""));
                    location_path = module;
                    error = *inner;
                }
                other => {
                    error = other;
                    break;
                }
            }
        }

        let position = error.take_position();
        let message = match error {
            EvalAltResult::ErrorTooManyOperations(..) => {
                format!("exceeded the operation budget of {operation_budget} per call (a runaway loop?)")
            }
            EvalAltResult::ErrorRuntime(value, ..) => value.to_string(),
            other => other.to_string(),
        };
        Self::Runtime {
            location: ScriptLocation::at(&location_path, position),
            message,
            call_stack: call_stack.join(" → "),
        }
    }
}
