// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! New workspace assets: every format opens in its own editor with content the runtime readers accept, and names follow
//! the naming rule.

use joy_editor::workspace::asset_types::{
    AssetEditor,
    asset_editor,
    create_asset_text,
    is_supported_asset_path,
    new_asset_path,
};
use joy_engine::gameplay::{
    parse_level,
    parse_object,
};
use joy_engine::particles::parse_particle_effect;
use joy_engine::serde_json::{
    self,
    Value,
    json,
};

#[test]
fn new_workspace_assets_open_in_their_editor_with_immediately_usable_content() {
    for extension in ["form", "formlab", "joyfx", "joyobject", "joylevel"] {
        let path = format!("assets/new.{extension}");
        assert!(is_supported_asset_path(&path));
        let text = create_asset_text(&path).expect("starter content");
        match extension {
            "form" => {
                assert!(text.starts_with("project meta(name=\"new\")"));
                assert!(!joy_engine::form::compile(&text, &json!({}), None).unwrap().triangles.is_empty());
            }
            "formlab" => {
                let document: Value = serde_json::from_str(&text).unwrap();
                assert_eq!(document["version"], 1);
                assert_eq!(document["overrides"], json!({}));
                let source = document["source"].as_str().unwrap();
                assert!(!joy_engine::form::compile(source, &json!({}), None).unwrap().triangles.is_empty());
            }
            "joyfx" => assert_eq!(parse_particle_effect(&text).unwrap().emitters().len(), 1),
            "joyobject" => {
                let object = parse_object(&text).unwrap();
                assert_eq!(object.entity.components["render"]["shape"], "box");
            }
            _ => assert!(parse_level(&text).unwrap().entities.is_empty()),
        }
        assert!(text.ends_with('\n'));
        let expected = match extension {
            "joyfx" | "form" | "formlab" => AssetEditor::Tool,
            _ => AssetEditor::Level,
        };
        assert_eq!(asset_editor(&path), expected);
    }
    assert_eq!(asset_editor("assets/unknown.txt"), AssetEditor::Source);
    assert!(!is_supported_asset_path("assets/unknown.txt"));
    assert!(create_asset_text("assets/unknown.txt").is_none());
}

#[test]
fn new_asset_names_use_safe_segments_and_add_their_extension() {
    assert_eq!(new_asset_path("tower", "formlab").unwrap(), "assets/tower.formlab");
    assert_eq!(new_asset_path("levels/track-1.joylevel", "joylevel").unwrap(), "assets/levels/track-1.joylevel");
    assert_eq!(new_asset_path("  spaced_name ", "joyfx").unwrap(), "assets/spaced_name.joyfx");
    assert_eq!(new_asset_path("", "joyfx").unwrap_err(), "Name the new asset before creating it.");
    for invalid in ["../escape", "with space", "a//b", "/rooted", "dots.in.name", "levels/"] {
        assert!(new_asset_path(invalid, "joylevel").is_err(), "{invalid} must be rejected");
    }
}
