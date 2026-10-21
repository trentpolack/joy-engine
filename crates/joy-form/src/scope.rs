// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Lexical variable scopes: script-level bindings per module, plus a stack of block frames.
//!
//! Every block (`if`, `repeat`, `wrangle`, `match`, and function bodies) pushes a frame, so a `let` inside a block ends
//! with it. Assignment (`name = value`) updates the nearest existing binding instead, which is how a loop accumulates
//! a count it reads afterwards. Functions see their own frames and their module's script-level bindings, and may read
//! but not assign script-level variables.

use crate::value::Value;
use std::collections::HashMap;

/// What a name is bound to and whether assignment may replace it.
#[derive(Clone, Debug)]
pub struct Binding {
    pub value: Value,
    pub role: BindingRole,
}

/// Why a binding exists; every role except [`BindingRole::Variable`] is read-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingRole {
    /// A `let` variable or a function parameter.
    Variable,
    /// A script parameter declared with `param`.
    Parameter,
    /// A `repeat` counter, a wrangle binding, or a wrangle's `index`.
    LoopValue,
    /// A built-in constant (`pi`, `tau`, `e`) or a sampled coordinate in `deform`.
    Constant,
    /// An `enum` or a `use` alias.
    Declaration,
}
impl BindingRole {
    /// How a read-only binding is described when assignment fails.
    pub fn description(self) -> &'static str {
        match self {
            Self::Variable => "variable",
            Self::Parameter => "parameter",
            Self::LoopValue => "loop value",
            Self::Constant => "constant",
            Self::Declaration => "declaration",
        }
    }
}

/// Script-level bindings of one module.
pub type Globals = HashMap<String, Binding>;

/// One block's bindings. Blocks bind a handful of names and are pushed for every loop iteration and call, so a short
/// list searched linearly costs less than hashing.
#[derive(Default)]
pub struct Frame {
    bindings: Vec<(String, Binding)>,
}

impl Frame {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind `name`, returning the binding it replaces.
    pub fn insert(&mut self, name: String, binding: Binding) -> Option<Binding> {
        match self.get_mut(&name) {
            Some(existing) => Some(std::mem::replace(existing, binding)),
            None => {
                self.bindings.push((name, binding));
                None
            }
        }
    }

    pub fn get(&self, name: &str) -> Option<&Binding> {
        self.bindings.iter().find(|(bound, _)| bound == name).map(|(_, binding)| binding)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Binding> {
        self.bindings.iter_mut().find(|(bound, _)| bound == name).map(|(_, binding)| binding)
    }
}

impl<const N: usize> From<[(String, Binding); N]> for Frame {
    fn from(bindings: [(String, Binding); N]) -> Self {
        Self {
            bindings: Vec::from(bindings),
        }
    }
}

/// The bindings visible to one statement: the frames of the current function or script body, innermost last.
pub struct Scope {
    /// The module whose script-level bindings sit beneath these frames.
    pub module: usize,
    pub frames: Vec<Frame>,
    /// Whether these frames belong to a function call, which may not assign script-level variables.
    pub in_function: bool,
}

impl Scope {
    /// The script body of `module`: no frames, so `let` declares script-level variables.
    pub fn script(module: usize) -> Self {
        Self {
            module,
            frames: Vec::new(),
            in_function: false,
        }
    }

    /// A function body of `module` whose first frame holds its parameters.
    pub fn function(module: usize, parameters: Frame) -> Self {
        Self {
            module,
            frames: vec![parameters],
            in_function: true,
        }
    }

    /// The innermost frame binding `name`, if any; script-level bindings are looked up separately.
    pub fn find(&self, name: &str) -> Option<&Binding> {
        self.frames.iter().rev().find_map(|frame| frame.get(name))
    }

    pub fn find_mut(&mut self, name: &str) -> Option<&mut Binding> {
        self.frames.iter_mut().rev().find_map(|frame| frame.get_mut(name))
    }
}
