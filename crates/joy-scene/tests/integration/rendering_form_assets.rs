// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM asset resolution for level presentation, using the engine's own authored structures.

use std::collections::HashMap;

use joy_core::math::Vec3;
use joy_gameplay::{
    EntityDefinition,
    component_type,
};
use joy_renderer::{
    RenderScene,
    ShadingModel,
};
use joy_scene::{
    AssetScene,
    FormAssetLibrary,
    FormPlacement,
    PLACEMENT_COMPONENT,
};
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

/// A grove script that asks for two posts at its tagged points and reports one decorative marker.
const GROVE_PATH: &str = "assets/form/grove.formlab";
const POST_PATH: &str = "assets/objects/post.joyobject";

fn grove_files() -> HashMap<String, String> {
    let source = r#"
        param spacing = 2 meta(min=1, max=8, step=1, kind="int")
        box(0, 0, 0, 4, 0.2, 4)
        with userData({kind: "post", object: "assets/objects/post.joyobject"}) {
            point(-spacing, 0, 0)
            point(spacing, 0, 0)
        }
        with userData({kind: "marker"}) { point(0, 1, 0) }
    "#;
    let grove = json!({
        "version": 1,
        "source": source,
        "variants": [{"name": "Wide", "overrides": {"spacing": 6}}]
    });
    let post = json!({
        "version": 1,
        "name": "Post",
        "entity": {"id": "object", "name": "Post", "components": {"render": {"shape": "box", "size": [0.2, 2, 0.2]}}}
    });
    HashMap::from([(GROVE_PATH.to_owned(), grove.to_string()), (POST_PATH.to_owned(), post.to_string())])
}

#[test]
fn form_object_placements_spawn_entities_that_follow_their_parent_and_variants_apply() {
    let files = grove_files();
    let read = |path: &str| files.get(path).cloned().ok_or_else(|| format!("missing {path}"));
    let mut grove = EntityDefinition::new("grove");
    grove.components.insert(
        component_type::RENDER.to_owned(),
        json!({"asset": GROVE_PATH, "variant": "wide"}).as_object().cloned().unwrap(),
    );

    let mut scene = RenderScene::new();
    let mut assets = AssetScene::new();
    let stage = assets.prepare(std::slice::from_ref(&grove), read).unwrap();
    let ids: Vec<&str> = stage.entities().iter().map(|entity| entity.id.as_str()).collect();
    assert_eq!(ids, ["grove", "grove/0", "grove/1"]);
    let post = &stage.entities()[2];
    assert!(post.template.is_none(), "spawned entities arrive with their templates resolved");
    assert_eq!(post.transform.position, Vec3::new(6.0, 0.0, 0.0), "the Wide variant spaces posts six meters out");
    assert!(post.components.contains_key(component_type::RENDER));
    assert_eq!(post.components[PLACEMENT_COMPONENT]["kind"], "post");
    assert!(assets.commit(&mut scene, stage, None).unwrap());
    assert_eq!(assets.placements("grove").len(), 3);
    assert_eq!(assets.placements("grove").iter().rev().find_map(FormPlacement::kind), Some("marker"));

    // Authored updates carry spawned entities with their parent.
    grove.transform.position = Vec3::new(0.0, 0.0, 10.0);
    assets.update(&mut scene, std::slice::from_ref(&grove), None).unwrap();
    let moved = assets.entities().iter().find(|entity| entity.id == "grove/1").unwrap();
    assert_eq!(moved.transform.position, Vec3::new(6.0, 0.0, 10.0));

    // A variant the document lacks is a load failure, not a silent fallback.
    grove.components[component_type::RENDER]["variant"] = json!("Missing");
    let error = assets.prepare(std::slice::from_ref(&grove), read).err().unwrap();
    assert!(error.to_string().contains("no FORM LAB variant named"), "{error}");
}
