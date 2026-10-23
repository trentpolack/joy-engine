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
fn serialized_documents_use_the_stable_key_order_and_keep_script_references() {
    let mut document = FormDocument {
        name: "Study".into(),
        source: "output(\"Slab\", box(1, 1, 1))".into(),
        script_path: None,
        overrides: json!({"size": 2}).as_object().unwrap().clone(),
        materials: None,
        variants: Some(Vec::new()),
    };
    assert_eq!(
        serialize_document(&document).unwrap(),
        r#"{"version":2,"name":"Study","source":"output(\"Slab\", box(1, 1, 1))","overrides":{"size":2},"variants":[]}"#
    );

    document.script_path = Some("../recipes/slab.form".into());
    let text = serialize_document(&document).unwrap();
    assert_eq!(
        text,
        r#"{"version":2,"name":"Study","form":"../recipes/slab.form","overrides":{"size":2},"variants":[]}"#
    );
    let read = |path: &str| -> Result<String, String> {
        assert_eq!(path, "assets/recipes/slab.form");
        Ok("output(\"Slab\", box(1, 1, 1))".into())
    };
    let reopened = read_instance_document(&text, "assets/studies/slab.forminst", Some(&read)).unwrap();
    assert_eq!(reopened, document);
    assert!(read_document(&text).unwrap_err().to_string().contains("Open it from its project folder"));
}

#[test]
fn version_one_formlab_documents_open_as_packed_instances() {
    let legacy = r#"{"version":1,"name":"Old","source":"param w = 2 meta(min=1, max=3, step=1)","overrides":{"w":3}}"#;
    let document = read_document(legacy).unwrap();
    assert_eq!((document.name.as_str(), document.script_path.as_ref()), ("Old", None));
    assert!(serialize_document(&document).unwrap().starts_with(r#"{"version":2,"name":"Old","source":"#));
}
