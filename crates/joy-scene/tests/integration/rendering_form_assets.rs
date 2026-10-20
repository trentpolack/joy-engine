// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM asset resolution for level presentation, using the engine's own authored structures.

use joy_renderer::ShadingModel;
use joy_scene::FormAssetLibrary;
use serde_json::{
    Map,
    json,
};

const COTTAGE_PATH: &str = "assets/form/structures/cottage.formlab";
const COTTAGE: &str = include_str!("../../../../assets/form/structures/cottage.formlab");
const BUILDING_PATH: &str = "assets/form/structures/building.form";
const BUILDING: &str = include_str!("../../../../assets/form/structures/building.form");

#[test]
fn form_lab_documents_compile_once_per_parameter_set() {
    let mut library = FormAssetLibrary::new();
    let none = Map::new();
    let cottage = library.resolve(COTTAGE_PATH, COTTAGE, &none, ShadingModel::Lit).unwrap();
    assert!(cottage.triangle_count() > 0);
    assert!(library.resolve(COTTAGE_PATH, COTTAGE, &none, ShadingModel::Lit).unwrap().ptr_eq(&cottage));

    let wider = json!({"width": 12}).as_object().cloned().unwrap();
    let resized = library.resolve(COTTAGE_PATH, COTTAGE, &wider, ShadingModel::Lit).unwrap();
    assert!(!resized.ptr_eq(&cottage));
    assert!(resized.bounds().max.x - resized.bounds().min.x > cottage.bounds().max.x - cottage.bounds().min.x);

    let building = library.resolve(BUILDING_PATH, BUILDING, &none, ShadingModel::Layered).unwrap();
    assert!(building.triangle_count() > 0);
}

#[test]
fn malformed_form_lab_documents_are_rejected() {
    let mut library = FormAssetLibrary::new();
    let none = Map::new();
    let error = library.resolve("assets/broken.formlab", r#"{"version": 2, "source": ""}"#, &none, ShadingModel::Lit);
    assert!(error.unwrap_err().to_string().contains("version"));
    let error = library.resolve("assets/broken.formlab", r#"{"version": 1}"#, &none, ShadingModel::Lit);
    assert!(error.unwrap_err().to_string().contains("source"));
    assert!(library.resolve("assets/broken.form", "this is not FORM (", &none, ShadingModel::Lit).is_err());
}
