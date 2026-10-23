// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/asset_path.rs`.

use super::*;

#[test]
fn rejects_paths_that_escape_the_asset_root() {
    assert_eq!(asset_path("assets/props/crate.joyobject", &[".joyobject"]), Ok("assets/props/crate.joyobject"));
    assert_eq!(asset_path("assets/Mesh.FORM", &[".form", ".forminst"]), Ok("assets/Mesh.FORM"));
    for unsafe_path in ["../outside.joyobject", "assets/../x", "assets//x", "assets/./x", "assets/a:b", "assets/a\u{1}"]
    {
        assert_eq!(asset_path(unsafe_path, &[]), Err(AssetPathError::OutsideAssets), "{unsafe_path:?}");
    }
    assert_eq!(
        asset_path("assets/crate.form", &[".joyobject"]),
        Err(AssetPathError::WrongExtension(".joyobject".to_owned()))
    );
}
