// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/lib.rs`.

use super::*;
use serde_json::json;

#[test]
fn project_name_tokens_locate_the_quoted_name() {
    let tokens = project_name_tokens("# x\nproject meta(info=\"A\", name=\"Old\")").unwrap();
    assert_eq!(
        tokens,
        [SourceToken {
            text: "\"Old\"".into(),
            line: 2,
            column: 29
        }]
    );
    assert!(project_name_tokens("point(").is_err());
    assert!(project_name_tokens("point(0,0,0)").unwrap().is_empty());
}

#[test]
fn parameter_values_validate_and_keep_integers() {
    assert_eq!(read_parameter_value(&json!(2.0)).unwrap().to_string(), "2");
    assert_eq!(read_parameter_value(&json!([1, 0.5, 0, 0.25])).unwrap(), json!([1, 0.5, 0, 0.25]));
    for invalid in [json!("2"), json!([1, 2]), json!([1, 2, null]), json!({"kind": "mesh"})] {
        assert!(read_parameter_value(&invalid).is_err(), "{invalid}");
    }
    let mesh = mesh_box_value(1.0, 1.0, 1.0).unwrap();
    assert_eq!(read_parameter_value(&mesh).unwrap(), mesh);
    assert_eq!(read_materials(None).unwrap(), vec![default_material()]);
}
