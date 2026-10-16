// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Inspector drafts for the level editor.
//!
//! Number text, validation, and Shift/Alt steps come from the authoring kit's UI-free rules
//! (`joy_engine::development::lab_values`). What the level editor adds is [`FieldDrafts`]: uncommitted text for every
//! inspector field (names, tags, transforms, component values) owned by the document model, so the workspace's saves
//! and the editor's selection changes can commit valid drafts and refuse while any draft is invalid, wherever the save
//! comes from. Scrolling over a field never steps it; egui fields ignore the wheel.

use std::collections::BTreeMap;

use joy_engine::development::lab_values::{self, NumericRules, StepScale};
use joy_engine::serde_json::{Number, Value};

/// Largest magnitude written as a JSON integer (JavaScript's safe integer range).
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Limits and arrow-key increment for one numeric field.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NumberBounds {
    /// Inclusive lower bound.
    pub min: Option<f64>,
    /// Inclusive upper bound.
    pub max: Option<f64>,
    /// Arrow-key increment.
    pub step: Option<f64>,
    /// Whole numbers only.
    pub integer: bool,
}

impl NumberBounds {
    /// The kit's validation rules for these bounds.
    pub fn rules(&self) -> NumericRules {
        NumericRules { minimum: self.min, maximum: self.max, integer: self.integer }
    }
}

/// Parse typed text as a number within `bounds`, or explain what to fix (the kit's draft validation).
pub fn parse_number(text: &str, bounds: &NumberBounds) -> Result<f64, String> {
    lab_values::validate_draft(text, &bounds.rules())
}

/// The value one arrow-key press produces (`direction` +1 or -1), clamped and rounded like the kit's controls.
pub fn step_number(value: f64, bounds: &NumberBounds, direction: f64, scale: StepScale) -> f64 {
    let step = bounds.step.unwrap_or(if bounds.integer { 1.0 } else { 0.1 });
    lab_values::stepped_value(value, step, scale, direction, &bounds.rules())
}

/// Text for a number: JavaScript's `String(number)`.
pub fn format_number(value: f64) -> String {
    lab_values::format_number(value)
}

/// Text for a displayed value rounded to four decimals, so untouched authored precision is never shown as an edit.
pub fn display_number(value: f64) -> String {
    format_number((value * 10_000.0).round() / 10_000.0)
}

/// The JSON number `JSON.stringify` writes: integral values become integers (`3`, never `3.0`); non-finite values are
/// `null`.
pub fn json_number(value: f64) -> Value {
    if !value.is_finite() {
        return Value::Null;
    }
    if value.fract() == 0.0 && value.abs() <= MAX_SAFE_INTEGER {
        return Value::from(value as i64);
    }
    Number::from_f64(value).map_or(Value::Null, Value::Number)
}

/// One uncommitted field: the typed text, how it validates, and, once a commit failed, why.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    /// What the author typed.
    pub text: String,
    /// Bounds for a numeric field; `None` for text.
    pub number: Option<NumberBounds>,
    /// The validation message from the last failed commit.
    pub error: Option<String>,
}

/// Uncommitted text for fields identified by `Key`, kept until a valid commit or an explicit discard.
#[derive(Clone, Debug)]
pub struct FieldDrafts<Key: Ord + Clone> {
    drafts: BTreeMap<Key, Draft>,
}

impl<Key: Ord + Clone> Default for FieldDrafts<Key> {
    fn default() -> Self {
        Self { drafts: BTreeMap::new() }
    }
}

impl<Key: Ord + Clone> FieldDrafts<Key> {
    /// No drafts.
    pub fn new() -> Self {
        Self::default()
    }

    /// The draft for `key`, if the author is editing it.
    pub fn get(&self, key: &Key) -> Option<&Draft> {
        self.drafts.get(key)
    }

    /// Record typed text for a text field; a new edit clears the previous error.
    pub fn set(&mut self, key: Key, text: impl Into<String>) {
        self.drafts.insert(key, Draft { text: text.into(), number: None, error: None });
    }

    /// Record typed text for a numeric field validated by `bounds`.
    pub fn set_number(&mut self, key: Key, text: impl Into<String>, bounds: NumberBounds) {
        self.drafts.insert(key, Draft { text: text.into(), number: Some(bounds), error: None });
    }

    /// Mark a draft invalid with a message, keeping its text visible.
    pub fn fail(&mut self, key: &Key, message: impl Into<String>) {
        if let Some(draft) = self.drafts.get_mut(key) {
            draft.error = Some(message.into());
        }
    }

    /// Forget one draft.
    pub fn discard(&mut self, key: &Key) {
        self.drafts.remove(key);
    }

    /// Forget every draft.
    pub fn clear(&mut self) {
        self.drafts.clear();
    }

    /// Every pending key, in order.
    pub fn keys(&self) -> Vec<Key> {
        self.drafts.keys().cloned().collect()
    }

    /// Whether nothing is pending.
    pub fn is_empty(&self) -> bool {
        self.drafts.is_empty()
    }

    /// The first draft whose last commit failed.
    pub fn first_invalid(&self) -> Option<(&Key, &Draft)> {
        self.drafts.iter().find(|(_, draft)| draft.error.is_some())
    }
}
