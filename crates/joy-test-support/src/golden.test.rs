// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/golden.rs`.

use serde_json::json;

use super::*;

#[test]
fn rerecording_keeps_numbers_that_still_agree_and_writes_changes() {
    let recorded = json!({ "label": "noon", "value": 0.1000000000001, "count": 3, "items": [1.5] });
    let produced = json!({ "label": "noon", "value": 0.1, "count": 4.0, "items": [1.5, 2.5], "added": true });
    let tolerance = Tolerance::relative(1e-9);
    let rerecorded = rerecord(&produced, &recorded, &tolerance);

    // The unchanged value keeps its recorded number and new data is added.
    assert_eq!(
        rerecorded,
        json!({ "label": "noon", "value": 0.1000000000001, "count": 4.0, "items": [1.5, 2.5], "added": true })
    );
    assert!(compare(&produced, &rerecorded, &tolerance).is_empty());

    // Recorded keys keep their order even when the code produces them in another.
    let reordered = json!({ "added": true, "items": [1.5, 2.5], "count": 4.0, "value": 0.1, "label": "noon" });
    let keys: Vec<String> = rerecord(&reordered, &recorded, &tolerance).as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, ["label", "value", "count", "items", "added"]);
}

#[test]
fn rewritten_files_keep_their_layout_and_number_spelling() {
    let directory = std::env::temp_dir().join(format!("joy-golden-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let recording = json!({ "cases": [{ "label": "a", "value": 1 }] });
    for (name, text) in [
        ("compact.json", "{\"cases\":[{\"label\":\"a\",\"value\":1}]}".to_owned()),
        ("numbers.json", "[0.000001,1e-7,-2.5e-8,1e+21,100000000000000000000,0.5,607.6541137695312]".to_owned()),
        ("indented.json", format!("{}\n", serde_json::to_string_pretty(&recording).unwrap())),
    ] {
        let path = directory.join(name);
        std::fs::write(&path, &text).unwrap();
        let (parsed, layout) = read(&path);
        write(&path, &parsed, layout);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text, "{name}");
    }
    std::fs::remove_dir_all(&directory).unwrap();
}
