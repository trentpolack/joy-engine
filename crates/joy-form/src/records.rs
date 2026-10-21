// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Record and list values built from literals (`{kind: "tower", seed: 4}`, `[400, 900]`) and their allocation
//! budget.

use crate::value::{
    SharedBudget,
    Value,
};
use serde_json::Value as Json;

pub fn copy(value: &Json, budget: &SharedBudget) -> Result<Json, String> {
    fn reserve_text(text: &str, budget: &SharedBudget) -> Result<(), String> {
        let mut budget = budget.borrow_mut();
        budget.user_data_bytes += text.len();
        if budget.user_data_bytes > 2_000_000 {
            return Err("Record allocation limit exceeded (text).".into());
        }
        Ok(())
    }
    fn visit(value: &Json, budget: &SharedBudget, depth: usize) -> Result<(), String> {
        {
            let mut budget = budget.borrow_mut();
            budget.user_data_values += 1;
            if depth > 32 || budget.user_data_values > 250_000 {
                return Err("Record allocation limit exceeded (depth or values).".into());
            }
        }
        match value {
            Json::String(s) => reserve_text(s, budget)?,
            Json::Array(items) => {
                for item in items {
                    visit(item, budget, depth + 1)?;
                }
            }
            Json::Object(fields) => {
                for (key, item) in fields {
                    validate_key(key)?;
                    reserve_text(key, budget)?;
                    visit(item, budget, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    visit(value, budget, 0)?;
    Ok(value.clone())
}
/// The record form of a value stored in a record or list literal.
pub fn data_value(value: &Value) -> Result<Json, String> {
    match value {
        Value::Number(_) | Value::Bool(_) | Value::Text(_) | Value::Array(_) | Value::Data(_) | Value::Enum(_) => {
            Ok(value.json())
        }
        other => Err(format!(
            "Records and lists hold numbers, true/false, text, vectors, colors, enum members, records, lists, and null; found {}.",
            other.describe()
        )),
    }
}
pub fn validate_key(name: &str) -> Result<(), String> {
    if name.is_empty() || name.chars().count() > 64 || crate::parser::RESERVED_DATA_KEYS.contains(&name) {
        Err("Invalid record field name (1–64 characters; prototype keys are reserved).".into())
    } else {
        Ok(())
    }
}
