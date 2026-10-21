// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use super::*;

#[test]
fn module_paths_resolve_beside_the_importer_and_stay_inside_the_root() {
    assert_eq!(resolve_path("parts/wall.form", Some("city/block.form")).unwrap(), "city/parts/wall.form");
    assert_eq!(resolve_path("../shared/kit.form", Some("city/block.form")).unwrap(), "shared/kit.form");
    assert_eq!(resolve_path("./kit.form", None).unwrap(), "kit.form");
    assert!(resolve_path("../../escape.form", Some("city/block.form")).is_err());
    assert!(resolve_path("/absolute.form", None).is_err());
    assert!(resolve_path("C:/drive.form", None).is_err());
    assert!(resolve_path("notes.txt", None).is_err());
}
