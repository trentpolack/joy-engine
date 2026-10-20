// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/document.rs`.

use super::*;
use joy_engine::serde_json::json;

#[test]
fn file_stems_keep_ascii_words() {
    assert_eq!(file_stem("Wave study"), "wave-study");
    assert_eq!(file_stem("  --Vessel  Workshop #2!"), "vessel-workshop-2");
    assert_eq!(file_stem("ÉÉ"), "form-study");
}

#[test]
fn serialized_documents_use_the_stable_key_order() {
    let document = FormDocument {
        name: "Study".into(),
        source: "point(0,0,0)".into(),
        overrides: json!({"size": 2}).as_object().unwrap().clone(),
        materials: None,
        variants: Some(Vec::new()),
    };
    assert_eq!(
        serialize_document(&document).unwrap(),
        r#"{"version":1,"name":"Study","source":"point(0,0,0)","overrides":{"size":2},"variants":[]}"#
    );
}
