// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/triangle_renderer.rs`.

use super::*;

#[test]
fn post_markers_expand_like_the_typescript_engine() {
    let source = "struct U {\n  a: vec4f,\n  // JOY_POST_UNIFORMS_MACRO\n};\n// JOY_POST_DECLARATIONS_MACRO\nfn f() -> vec4f {\n  let sourceColor = vec4f(1.0);\n  // JOY_POST_APPLY_MACRO\n}";
    let expanded = expand_post_markers(source);
    assert!(!expanded.contains("JOY_POST"));
    assert!(expanded.contains("return sourceColor;"));
    assert_eq!(expand_post_markers("plain"), "plain");
}
