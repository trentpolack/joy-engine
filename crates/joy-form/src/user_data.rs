// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Authored user-data records and their allocation budget. Ported from `src/form/user-data.ts`.

use crate::value::{
    DataRef,
    SharedBudget,
    Value,
};
use serde_json::Value as Json;

pub fn copy(value: &Json, budget: &SharedBudget) -> Result<Json, String> {
    fn reserve_text(text: &str, budget: &SharedBudget) -> Result<(), String> {
        let mut budget = budget.borrow_mut();
        budget.user_data_bytes += text.encode_utf16().count();
        if budget.user_data_bytes > 2_000_000 {
            return Err("User data allocation limit exceeded (text).".into());
        }
        Ok(())
    }
    fn visit(value: &Json, budget: &SharedBudget, depth: usize) -> Result<(), String> {
        {
            let mut budget = budget.borrow_mut();
            budget.user_data_values += 1;
            if depth > 32 || budget.user_data_values > 250_000 {
                return Err("User data allocation limit exceeded (depth or values).".into());
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
pub fn data_value(value: &Value) -> Result<Json, String> {
    match value {
        Value::Number(_) | Value::Text(_) | Value::Array(_) | Value::Data(_) =>Ok(value.json()),
        _=>Err("User data accepts records, arrays, text, finite numbers, booleans, and null; resource handles are not data.".into())
    }
}
pub fn data_record(value: &Value) -> Result<DataRef, String> {
    if let Value::Data(record) = value {
        if record.is_null() || record.is_object() {
            return Ok(record.clone());
        }
    } else {
        // Preserve the distinct resource-handle diagnostic without copying data.
        data_value(value)?;
    }
    Err("userData expects a record or null.".into())
}
pub fn validate_key(name: &str) -> Result<(), String> {
    if name.is_empty() || name.encode_utf16().count() > 64 || ["__proto__", "prototype", "constructor"].contains(&name)
    {
        Err("Invalid user data field name (1–64 characters; prototype keys are reserved).".into())
    } else {
        Ok(())
    }
}
