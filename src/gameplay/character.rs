// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Control intent and possession contracts for entities.
//!
//! Entities keep plain [`EntityState`](super::EntityState) and optional [`EntityBehavior`](super::EntityBehavior).
//! Possession and intent are split across those owners:
//! - [`EntityState::possessed_by`](super::EntityState::possessed_by) records the controller, maintained by
//!   [`EntityWorld::possess`](super::EntityWorld::possess).
//! - [`EntityState::control_intent`](super::EntityState::control_intent) stores a [`ControlIntent`].
//! - [`EntityWorld::receive_control`](super::EntityWorld::receive_control) accepts intent;
//!   [`ControlIntent::from_json`] validates untrusted records.
//! - [`EntityWorld::reset_control`](super::EntityWorld::reset_control) clears intent and calls the
//!   [`EntityBehavior::reset_control`](super::EntityBehavior::reset_control) hook.
//! - [`EntityWorld::remove`](super::EntityWorld::remove) releases possession before removing an entity.
//!
//! The built-in `character` and `player-character` types ([`entity_type`](super::entity_type)) remain registered, and
//! every entity type may be possessed.

use indexmap::IndexMap;
use serde_json::{
    Map,
    Value,
};

use super::error::GameplayError;
use crate::core::format::json_number;

/// One named action value: an axis or a button.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ControlValue {
    /// Analog value, for example throttle in `[-1, 1]`. Must be finite when delivered.
    Number(f64),
    /// Digital value, for example a boost button.
    Bool(bool),
}

impl ControlValue {
    /// Numeric reading: numbers as-is, `true` as 1, and `false` as 0.
    pub fn as_number(self) -> f64 {
        match self {
            Self::Number(value) => value,
            Self::Bool(flag) => f64::from(u8::from(flag)),
        }
    }

    /// Boolean reading: booleans as-is and numbers as "nonzero".
    pub fn as_bool(self) -> bool {
        match self {
            Self::Number(value) => value != 0.0,
            Self::Bool(flag) => flag,
        }
    }
}

impl From<f64> for ControlValue {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}

impl From<f32> for ControlValue {
    fn from(value: f32) -> Self {
        Self::Number(f64::from(value))
    }
}

impl From<bool> for ControlValue {
    fn from(flag: bool) -> Self {
        Self::Bool(flag)
    }
}

/// Project-defined actions sampled by a controller, in insertion order. Each delivery replaces the whole intent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ControlIntent {
    values: IndexMap<String, ControlValue>,
}

impl ControlIntent {
    /// An empty intent.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder form of [`ControlIntent::insert`].
    pub fn with(mut self, name: impl Into<String>, value: impl Into<ControlValue>) -> Self {
        self.insert(name, value);
        self
    }

    /// Set one action, replacing any previous value for the name.
    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<ControlValue>) {
        self.values.insert(name.into(), value.into());
    }

    /// The raw value of an action, if present.
    pub fn get(&self, name: &str) -> Option<ControlValue> {
        self.values.get(name).copied()
    }

    /// Numeric reading of an action: missing actions read 0, booleans read 0 or 1.
    pub fn number(&self, name: &str) -> f64 {
        self.get(name).map_or(0.0, ControlValue::as_number)
    }

    /// Boolean reading of an action: missing actions read false, numbers read "nonzero".
    pub fn flag(&self, name: &str) -> bool {
        self.get(name).is_some_and(ControlValue::as_bool)
    }

    /// Whether no actions are set.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Number of actions set.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Remove every action.
    pub fn clear(&mut self) {
        self.values.clear();
    }

    /// Actions in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, ControlValue)> {
        self.values.iter().map(|(name, value)| (name.as_str(), *value))
    }

    /// Fail if any numeric action is not finite, matching the TypeScript `receiveControl` check.
    pub fn validate(&self) -> Result<(), GameplayError> {
        let finite = self.values.values().all(|value| match value {
            ControlValue::Number(number) => number.is_finite(),
            ControlValue::Bool(_) => true,
        });
        if !finite {
            return Err(GameplayError::NonFiniteIntent);
        }
        Ok(())
    }

    /// Copy an untrusted intent record (for example a replay or editor preview message), like `receiveControl`.
    ///
    /// The value must be a JSON object whose values are all numbers or booleans; key order is preserved. Fails with
    /// [`GameplayError::InvalidIntentRecord`] for a non-object and [`GameplayError::NonFiniteIntent`] for any other
    /// value type.
    pub fn from_json(value: &Value) -> Result<Self, GameplayError> {
        let Value::Object(record) = value else {
            return Err(GameplayError::InvalidIntentRecord);
        };
        let mut intent = Self::new();
        for (name, entry) in record {
            let action = match entry {
                Value::Bool(flag) => ControlValue::Bool(*flag),
                Value::Number(number) => ControlValue::Number(number.as_f64().ok_or(GameplayError::NonFiniteIntent)?),
                _ => return Err(GameplayError::NonFiniteIntent),
            };
            intent.values.insert(name.clone(), action);
        }
        Ok(intent)
    }

    /// The intent as a JSON record in insertion order, numbers written as JavaScript would.
    pub fn to_json(&self) -> Value {
        let record: Map<String, Value> = self
            .values
            .iter()
            .map(|(name, value)| {
                let entry = match value {
                    ControlValue::Number(number) => json_number(*number),
                    ControlValue::Bool(flag) => Value::Bool(*flag),
                };
                (name.clone(), entry)
            })
            .collect();
        Value::Object(record)
    }
}

impl<K: Into<String>, V: Into<ControlValue>> FromIterator<(K, V)> for ControlIntent {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut intent = Self::new();
        for (name, value) in iter {
            intent.insert(name, value);
        }
        intent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn intent_records_round_trip_and_reject_non_actions() {
        let intent = ControlIntent::from_json(&json!({"throttle": 1, "brake": false, "steer": -0.5})).unwrap();
        assert_eq!(intent, ControlIntent::new().with("throttle", 1.0).with("brake", false).with("steer", -0.5));
        assert_eq!(intent.to_json().to_string(), r#"{"throttle":1,"brake":false,"steer":-0.5}"#);
        assert_eq!(ControlIntent::from_json(&json!([1])), Err(GameplayError::InvalidIntentRecord));
        assert_eq!(ControlIntent::from_json(&json!({"throttle": "full"})), Err(GameplayError::NonFiniteIntent));
    }
}
