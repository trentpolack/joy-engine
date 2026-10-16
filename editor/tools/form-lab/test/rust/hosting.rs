// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM LAB as a Joy Editor tab through `AuthoringTool`: the editor owns files, `.form` saves keep the raw script
//! exactly, `.formlab` saves keep overrides, materials, meshes, and variants, and the tab draws inside its area.

use form_lab::FormLab;
use joy_engine::development::{AuthoringTool, ToolDocument, ToolHosting};
use joy_engine::egui;
use joy_engine::serde_json::json;

fn hosted() -> FormLab {
    let mut lab = FormLab::with_storage(ToolHosting::Hosted { area: egui::Rect::NOTHING }, None);
    lab.set_hosting(ToolHosting::Hosted {
        area: egui::Rect::from_min_size(egui::pos2(0.0, 40.0), egui::vec2(1200.0, 700.0)),
    });
    lab
}

#[test]
fn form_scripts_save_exactly_as_written_and_overrides_stay_temporary() {
    let mut lab = hosted();
    assert_eq!(lab.tool_name(), "FORM LAB");
    assert_eq!(lab.document_extensions(), ["form", "formlab"]);
    let source =
        "# keep   my spacing\nparam height = 2 meta(min=0, max=5, step=0.5)\nbox(0, height/2, 0, 1, height, 1)\n";
    lab.open_document(ToolDocument { name: "props/tower.form".into(), contents: source.into() }).unwrap();
    assert!(!lab.has_unsaved_changes());
    lab.finish_build();
    assert!(lab.session().data().is_some(), "{:?}", lab.session().status());
    lab.session_mut().set_override("height", Some(json!(4)));
    assert!(lab.has_unsaved_changes());
    assert_eq!(lab.save_document("props/tower.form").unwrap(), source, "a raw save never bakes overrides");
    let document: joy_engine::serde_json::Value =
        joy_engine::serde_json::from_str(&lab.save_document("props/tower.formlab").unwrap()).unwrap();
    assert_eq!(document["overrides"], json!({"height": 4}));
    assert_eq!(document["source"], source);
    lab.mark_saved();
    assert!(!lab.has_unsaved_changes());
}

#[test]
fn formlab_documents_round_trip_their_materials_meshes_and_variants() {
    let mut lab = hosted();
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test/rust/fixtures/variant-study.formlab"),
    )
    .unwrap();
    lab.open_document(ToolDocument { name: "variant-study.formlab".into(), contents: text.clone() }).unwrap();
    let saved = lab.save_document("variant-study.formlab").unwrap();
    let original: joy_engine::serde_json::Value = joy_engine::serde_json::from_str(&text).unwrap();
    let round_trip: joy_engine::serde_json::Value = joy_engine::serde_json::from_str(&saved).unwrap();
    assert_eq!(round_trip, original, "nothing is lost or reformatted in meaning");
    assert_eq!(saved, text, "compact JSON in the TypeScript key order");
    assert!(lab.open_document(ToolDocument { name: "broken.formlab".into(), contents: "{".into() }).is_err());
    assert_eq!(lab.session().name, "Variant study", "a rejected document leaves the current one in place");
    assert!(!lab.session().recovers(), "hosted tabs never write browser recovery");
}
