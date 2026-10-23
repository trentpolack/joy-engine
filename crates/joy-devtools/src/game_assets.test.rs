// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use super::*;

#[test]
fn embedded_reads_refuse_paths_outside_assets() {
    let mut assets = GameAssets::new([("assets/tower.form", "box(0,0,0,1,1,1)")], None);
    assert!(!assets.hot_reload());
    assert_eq!(assets.read("assets/tower.form").unwrap(), "box(0,0,0,1,1,1)");
    assert_eq!(assets.read("assets/missing.form"), Err(AssetReadError::Missing("assets/missing.form".into())));
    for path in ["../secrets.txt", "assets/../Cargo.toml", "/etc/hosts", "assets//tower.form", "src/lib.rs"] {
        assert_eq!(assets.read(path), Err(AssetReadError::OutsideAssets(path.into())));
    }
    assert!(assets.poll_changes(10.0).is_empty());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn development_reads_prefer_source_files_and_report_edits_once_polled() {
    let root = std::env::temp_dir().join(format!("joy-game-assets-{}", std::process::id()));
    let folder = root.join("assets");
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join("tower.form");
    std::fs::write(&file, "box(0,0,0,2,2,2)").unwrap();

    let mut assets = GameAssets::new([("assets/tower.form", "embedded")], root.to_str());
    assert!(assets.hot_reload());
    assert_eq!(assets.read("assets/tower.form").unwrap(), "box(0,0,0,2,2,2)");
    assert!(assets.poll_changes(ASSET_POLL_SECONDS).is_empty());

    // A longer file changes the stamp even when the modification time has coarse resolution.
    std::fs::write(&file, "box(0,0,0,3,3,3) // taller").unwrap();
    assert!(assets.poll_changes(ASSET_POLL_SECONDS * 0.5).is_empty(), "polls wait for the interval");
    assert_eq!(assets.poll_changes(ASSET_POLL_SECONDS), ["assets/tower.form"]);
    assert_eq!(assets.read("assets/tower.form").unwrap(), "box(0,0,0,3,3,3) // taller");
    assert!(assets.poll_changes(ASSET_POLL_SECONDS).is_empty(), "reading again settles the change");

    // A removed file falls back to the embedded text.
    std::fs::remove_file(&file).unwrap();
    assert_eq!(assets.read("assets/tower.form").unwrap(), "embedded");
    let _ = std::fs::remove_dir_all(&root);
}
