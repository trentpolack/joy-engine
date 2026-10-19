// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/document.rs`.

use super::*;

#[test]
fn identifiers_match_whole_words_only() {
    assert!(contains_identifier("size = power * 2;", "power"));
    assert!(!contains_identifier("size = powerful;", "power"));
    assert!(!contains_identifier("size = _power;", "power"));
    assert!(contains_identifier("power", "power"));
    assert_eq!(group_thousands(6_000_000), "6,000,000");
    assert_eq!(file_stem("Ember Impact!"), "ember-impact");
    assert_eq!(file_stem("  ***  "), "particle-effect");
    assert_eq!(export_file_name("Solar Swirl 2"), "solar-swirl-2.joyfx");
}
