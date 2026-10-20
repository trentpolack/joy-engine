// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Console variables: named, typed runtime settings a development console reads and writes, after Unreal Engine's
//! cvars. The engine registers its `r.` rendering variables (see `render_variables.rs`); games register their own and
//! read them each frame. Every value is checked against its variable's kind and range, names compare without regard to
//! case, and a change is queued once so its owner can apply it.

use std::collections::BTreeMap;

/// What values a variable accepts.
#[derive(Clone, Debug, PartialEq)]
pub enum VariableKind {
    /// On or off: `1`, `0`, `true`, `false`, `on`, or `off`.
    Bool,
    /// A whole number within an inclusive range.
    Integer {
        /// Smallest accepted value.
        minimum: i64,
        /// Largest accepted value.
        maximum: i64,
    },
    /// A number within an inclusive range.
    Float {
        /// Smallest accepted value.
        minimum: f64,
        /// Largest accepted value.
        maximum: f64,
    },
    /// One of a fixed set of names, compared without regard to case.
    Choice(Vec<String>),
    /// Any text.
    Text,
}

/// A variable's value.
#[derive(Clone, Debug, PartialEq)]
pub enum VariableValue {
    /// For [`VariableKind::Bool`].
    Bool(bool),
    /// For [`VariableKind::Integer`].
    Integer(i64),
    /// For [`VariableKind::Float`].
    Float(f64),
    /// For [`VariableKind::Choice`] (the choice's own spelling) and [`VariableKind::Text`].
    Text(String),
}

impl std::fmt::Display for VariableValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool(value) => write!(formatter, "{}", u8::from(*value)),
            Self::Integer(value) => write!(formatter, "{value}"),
            Self::Float(value) => write!(formatter, "{value}"),
            Self::Text(value) => write!(formatter, "{value}"),
        }
    }
}

/// One variable's name, help, kind, and default.
#[derive(Clone, Debug, PartialEq)]
pub struct VariableDefinition {
    /// Dotted name, such as `r.ScreenPercentage`; letters, digits, dots, and underscores.
    pub name: String,
    /// One line describing what the variable does, with its units.
    pub help: String,
    /// What values it accepts.
    pub kind: VariableKind,
    /// Its value before anything sets it, and after a reset.
    pub default: VariableValue,
}

impl VariableDefinition {
    /// An on/off variable.
    pub fn bool(name: &str, help: &str, default: bool) -> Self {
        Self::new(name, help, VariableKind::Bool, VariableValue::Bool(default))
    }

    /// A whole-number variable within `minimum..=maximum`.
    pub fn integer(name: &str, help: &str, default: i64, minimum: i64, maximum: i64) -> Self {
        Self::new(
            name,
            help,
            VariableKind::Integer {
                minimum,
                maximum,
            },
            VariableValue::Integer(default),
        )
    }

    /// A numeric variable within `minimum..=maximum`.
    pub fn float(name: &str, help: &str, default: f64, minimum: f64, maximum: f64) -> Self {
        Self::new(
            name,
            help,
            VariableKind::Float {
                minimum,
                maximum,
            },
            VariableValue::Float(default),
        )
    }

    /// A variable naming one of `choices`.
    pub fn choice(name: &str, help: &str, choices: &[&str], default: &str) -> Self {
        let choices = choices.iter().map(|choice| (*choice).to_owned()).collect();
        Self::new(name, help, VariableKind::Choice(choices), VariableValue::Text(default.to_owned()))
    }

    /// A free-text variable.
    pub fn text(name: &str, help: &str, default: &str) -> Self {
        Self::new(name, help, VariableKind::Text, VariableValue::Text(default.to_owned()))
    }

    fn new(name: &str, help: &str, kind: VariableKind, default: VariableValue) -> Self {
        Self {
            name: name.to_owned(),
            help: help.to_owned(),
            kind,
            default,
        }
    }

    /// What the variable accepts, for help text: `0 or 1`, `25 to 100`, or the choices.
    pub fn accepted(&self) -> String {
        match &self.kind {
            VariableKind::Bool => "0 or 1".to_owned(),
            VariableKind::Integer {
                minimum,
                maximum,
            } => format!("{minimum} to {maximum}"),
            VariableKind::Float {
                minimum,
                maximum,
            } => format!("{minimum} to {maximum}"),
            VariableKind::Choice(choices) => choices.join(", "),
            VariableKind::Text => "text".to_owned(),
        }
    }

    /// Parse `text` as a value of this variable.
    pub fn parse(&self, text: &str) -> Result<VariableValue, ConsoleError> {
        let text = text.trim();
        let invalid = || ConsoleError::Invalid {
            name: self.name.clone(),
            expected: self.accepted(),
        };
        match &self.kind {
            VariableKind::Bool => match text.to_ascii_lowercase().as_str() {
                "1" | "true" | "on" => Ok(VariableValue::Bool(true)),
                "0" | "false" | "off" => Ok(VariableValue::Bool(false)),
                _ => Err(invalid()),
            },
            VariableKind::Integer {
                ..
            } => text.parse().map(VariableValue::Integer).map_err(|_| invalid()),
            VariableKind::Float {
                ..
            } => text.parse().map(VariableValue::Float).map_err(|_| invalid()),
            VariableKind::Choice(choices) => choices
                .iter()
                .find(|choice| choice.eq_ignore_ascii_case(text))
                .map(|choice| VariableValue::Text(choice.clone()))
                .ok_or_else(invalid),
            VariableKind::Text => Ok(VariableValue::Text(text.to_owned())),
        }
        .and_then(|value| self.check(value))
    }

    /// `value` when it fits this variable's kind and range.
    pub fn check(&self, value: VariableValue) -> Result<VariableValue, ConsoleError> {
        let fits = match (&self.kind, &value) {
            (VariableKind::Bool, VariableValue::Bool(_)) => true,
            (
                VariableKind::Integer {
                    minimum,
                    maximum,
                },
                VariableValue::Integer(number),
            ) => (*minimum..=*maximum).contains(number),
            (
                VariableKind::Float {
                    minimum,
                    maximum,
                },
                VariableValue::Float(number),
            ) => number.is_finite() && (*minimum..=*maximum).contains(number),
            (VariableKind::Choice(choices), VariableValue::Text(text)) => choices.contains(text),
            (VariableKind::Text, VariableValue::Text(_)) => true,
            _ => false,
        };
        if fits {
            Ok(value)
        } else {
            Err(ConsoleError::Invalid {
                name: self.name.clone(),
                expected: self.accepted(),
            })
        }
    }
}

/// A console variable request that failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConsoleError {
    /// No variable has this name.
    #[error("Unknown console variable '{0}'.")]
    Unknown(String),
    /// A variable with this name is already registered.
    #[error("Console variable '{0}' is already registered.")]
    Duplicate(String),
    /// Names use letters, digits, dots, and underscores, and start with a letter.
    #[error("'{0}' is not a valid console variable name.")]
    InvalidName(String),
    /// The value does not fit the variable.
    #[error("{name} accepts {expected}.")]
    Invalid {
        /// The variable's name.
        name: String,
        /// What it accepts.
        expected: String,
    },
}

/// A registered variable and its current value.
#[derive(Clone, Debug)]
struct Variable {
    definition: VariableDefinition,
    value: VariableValue,
}

/// The registered console variables, keyed by lowercase name.
#[derive(Clone, Debug, Default)]
pub struct ConsoleVariables {
    variables: BTreeMap<String, Variable>,
    /// Names changed since their owners last took changes, in change order without repeats.
    changed: Vec<String>,
}

impl ConsoleVariables {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a variable at its default value.
    pub fn register(&mut self, definition: VariableDefinition) -> Result<(), ConsoleError> {
        if !valid_name(&definition.name) {
            return Err(ConsoleError::InvalidName(definition.name));
        }
        let key = definition.name.to_ascii_lowercase();
        if self.variables.contains_key(&key) {
            return Err(ConsoleError::Duplicate(definition.name));
        }
        let value = definition.check(definition.default.clone())?;
        self.variables.insert(
            key,
            Variable {
                definition,
                value,
            },
        );
        Ok(())
    }

    /// A variable's current value.
    pub fn get(&self, name: &str) -> Option<&VariableValue> {
        self.variable(name).map(|variable| &variable.value)
    }

    /// A [`VariableKind::Bool`] variable's value.
    pub fn bool(&self, name: &str) -> Option<bool> {
        match self.get(name)? {
            VariableValue::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// A [`VariableKind::Integer`] variable's value.
    pub fn integer(&self, name: &str) -> Option<i64> {
        match self.get(name)? {
            VariableValue::Integer(value) => Some(*value),
            _ => None,
        }
    }

    /// A [`VariableKind::Float`] variable's value.
    pub fn float(&self, name: &str) -> Option<f64> {
        match self.get(name)? {
            VariableValue::Float(value) => Some(*value),
            _ => None,
        }
    }

    /// A [`VariableKind::Choice`] or [`VariableKind::Text`] variable's value.
    pub fn text(&self, name: &str) -> Option<&str> {
        match self.get(name)? {
            VariableValue::Text(value) => Some(value),
            _ => None,
        }
    }

    /// A variable's definition.
    pub fn definition(&self, name: &str) -> Option<&VariableDefinition> {
        self.variable(name).map(|variable| &variable.definition)
    }

    /// Parse and set a variable from console text; returns the new value. A changed value is queued for its owner.
    pub fn set(&mut self, name: &str, text: &str) -> Result<VariableValue, ConsoleError> {
        let definition = self.definition(name).ok_or_else(|| ConsoleError::Unknown(name.to_owned()))?;
        let value = definition.parse(text)?;
        self.store(name, value.clone(), true);
        Ok(value)
    }

    /// Set a variable to a typed value; a changed value is queued for its owner.
    pub fn set_value(&mut self, name: &str, value: VariableValue) -> Result<(), ConsoleError> {
        let definition = self.definition(name).ok_or_else(|| ConsoleError::Unknown(name.to_owned()))?;
        let value = definition.check(value)?;
        self.store(name, value, true);
        Ok(())
    }

    /// Show the state a variable's owner is actually in without queueing a change, so the console reports what runs
    /// (a game may change the renderer itself). Values that do not fit are ignored.
    pub fn mirror(&mut self, name: &str, value: VariableValue) {
        let fits = self.definition(name).is_some_and(|definition| definition.check(value.clone()).is_ok());
        if fits {
            self.store(name, value, false);
        }
    }

    /// Return a variable to its default; queued like any change.
    pub fn reset(&mut self, name: &str) -> Result<VariableValue, ConsoleError> {
        let default = self.definition(name).ok_or_else(|| ConsoleError::Unknown(name.to_owned()))?.default.clone();
        self.store(name, default.clone(), true);
        Ok(default)
    }

    /// Names of the variables whose values changed since the last call with this `prefix` (such as `r.`), in change
    /// order. Changes to other variables stay queued for their owners.
    pub fn take_changes(&mut self, prefix: &str) -> Vec<String> {
        let prefix = prefix.to_ascii_lowercase();
        let (taken, kept): (Vec<String>, Vec<String>) =
            std::mem::take(&mut self.changed).into_iter().partition(|key| key.starts_with(&prefix));
        self.changed = kept;
        taken
            .iter()
            .filter_map(|key| self.variables.get(key))
            .map(|variable| variable.definition.name.clone())
            .collect()
    }

    /// Every definition, sorted by name.
    pub fn definitions(&self) -> impl Iterator<Item = &VariableDefinition> {
        self.variables.values().map(|variable| &variable.definition)
    }

    /// Definitions whose name or help contains `text`, ignoring case.
    pub fn find(&self, text: &str) -> Vec<&VariableDefinition> {
        let text = text.to_ascii_lowercase();
        self.definitions()
            .filter(|definition| {
                definition.name.to_ascii_lowercase().contains(&text)
                    || definition.help.to_ascii_lowercase().contains(&text)
            })
            .collect()
    }

    /// Names starting with `prefix`, ignoring case, sorted.
    pub fn complete(&self, prefix: &str) -> Vec<&str> {
        let prefix = prefix.to_ascii_lowercase();
        self.variables
            .iter()
            .filter(|(key, _)| key.starts_with(&prefix))
            .map(|(_, variable)| variable.definition.name.as_str())
            .collect()
    }

    fn variable(&self, name: &str) -> Option<&Variable> {
        self.variables.get(&name.to_ascii_lowercase())
    }

    fn store(&mut self, name: &str, value: VariableValue, queue: bool) {
        let key = name.to_ascii_lowercase();
        let Some(variable) = self.variables.get_mut(&key) else {
            return;
        };
        if variable.value == value {
            return;
        }
        variable.value = value;
        if queue && !self.changed.contains(&key) {
            self.changed.push(key);
        }
    }
}

/// Letters, digits, dots, and underscores, starting with a letter.
fn valid_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters.next().is_some_and(|first| first.is_ascii_alphabetic())
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '.' || character == '_')
}

#[cfg(test)]
#[path = "console_variables.test.rs"]
mod tests;
