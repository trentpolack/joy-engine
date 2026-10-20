// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The level editor's document logic: transactions with undo, primitives, transforms in degrees, schema-driven
//! properties with linked-object overrides, invalid input that blocks saving, reusable objects, picking, and the region
//! camera.

use std::collections::HashMap;

use joy_editor::level::property_editor::{
    CustomFieldKind,
    FieldControl,
    PropertyContext,
    PropertyNode,
    add_custom_field,
    append_list_item,
    build_property_tree,
};
use joy_editor::level::viewport::{
    LevelViewport,
    ViewRegion,
    pick_entity,
};
use joy_editor::level::{
    AssetAccess,
    ComponentChoice,
    FieldKey,
    LevelEditor,
    TransformField,
};
use joy_editor::server::AssetEntry;
use joy_engine::gameplay::{
    EntityDefinition,
    parse_level,
    parse_object,
};
use joy_engine::math::{
    Vec2,
    Vec3,
};
use joy_engine::rendering::{
    Camera,
    OrbitCamera,
    WorldRay,
    engine_component_catalog,
    project_to_viewport,
};
use joy_engine::serde_json::{
    Map,
    Value,
    json,
};

use crate::support::level_text;

/// Project content held in memory.
#[derive(Default)]
pub struct MemoryAccess {
    pub files: Vec<AssetEntry>,
    pub texts: HashMap<String, String>,
    pub created: Vec<String>,
}

impl MemoryAccess {
    pub fn with(mut self, path: &str, text: &str) -> Self {
        let kind = path.rsplit('.').next().unwrap_or_default().to_owned();
        self.files.push(AssetEntry { path: path.to_owned(), kind });
        self.texts.insert(path.to_owned(), text.to_owned());
        self
    }
}

impl AssetAccess for MemoryAccess {
    fn catalog(&self) -> &[AssetEntry] {
        &self.files
    }

    fn read(&mut self, path: &str) -> Result<String, String> {
        self.texts.get(path).cloned().ok_or_else(|| format!("Cannot open {path}"))
    }

    fn create(&mut self, path: &str, text: &str) -> Result<(), String> {
        if self.texts.contains_key(path) {
            return Err("A file with this name already exists.".to_owned());
        }
        self.created.push(path.to_owned());
        *self = std::mem::take(self).with(path, text);
        Ok(())
    }
}

fn editor() -> LevelEditor {
    LevelEditor::load("assets/track.joylevel", &level_text("Track")).unwrap()
}

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

#[test]
fn levels_round_trip_unknown_components_and_report_invalid_documents() {
    let text = level_text("Track");
    let mut level = LevelEditor::load("assets/track.joylevel", &text).unwrap();
    assert_eq!(level.capture(false).unwrap(), text, "an untouched level serializes byte for byte");
    assert!(LevelEditor::load("assets/broken.joylevel", "{broken").is_err());
    let error = |path: &str, text: &str| LevelEditor::load(path, text).err().unwrap_or_default();
    assert!(error("assets/old.joylevel", r#"{"version": 2, "name": "x", "entities": []}"#).contains("version"));
    let duplicate = r#"{"version": 1, "name": "x", "entities": [{"id": "a"}, {"id": "a"}]}"#;
    assert!(error("assets/duplicate.joylevel", duplicate).to_lowercase().contains("duplicate"));
}

#[test]
fn primitives_duplicates_deletes_and_history_are_document_transactions() {
    let mut level = editor();
    level.viewport.camera.target = Vec3::new(2.4, 0.0, -3.6);
    level.add_primitive(false);
    let added = level.selected().cloned().unwrap();
    assert_eq!(added.name, "Box");
    assert_eq!(added.transform.position, Vec3::new(2.0, 0.5, -4.0), "placement snaps to the one-meter grid");
    assert_eq!(added.components["render"]["shape"], "box");
    assert_eq!(added.components["physics"]["shape"], "box");
    assert!(level.status().contains("Primitive added"));
    level.add_primitive(true);
    assert_eq!(level.selected().unwrap().components["physics"]["shape"], "sphere");
    level.duplicate();
    let copy = level.selected().cloned().unwrap();
    assert_eq!(copy.name, "Sphere copy");
    assert_eq!(copy.transform.position.x, 3.0);
    assert_eq!(level.document().level().entities.len(), 4);
    level.history(false);
    assert_eq!(level.document().level().entities.len(), 3);
    level.history(true);
    assert_eq!(level.document().level().entities.len(), 4);
    assert!(level.selected().is_none(), "undo clears a selection whose entity disappeared");
    level.select(Some(&copy.id));
    level.remove_selected();
    assert_eq!(level.document().level().entities.len(), 3);
    assert!(level.selected().is_none());
    level.add_light();
    let light = level.selected().unwrap();
    assert_eq!(light.transform.position.y, 3.0);
    assert!(light.components.contains_key("light"));
    level.add_empty();
    assert!(level.selected().unwrap().components.is_empty());
    let saved = parse_level(&level.capture(false).unwrap()).unwrap();
    assert_eq!(saved.entities.len(), 5);
}

#[test]
fn transforms_edit_in_degrees_store_radians_and_keep_untouched_precision() {
    let mut level = editor();
    level.select(Some("box"));
    let key = FieldKey::Transform(TransformField::Rotation, 1);
    level.drafts_mut().set_number(key.clone(), "90", Default::default());
    level.commit_draft(&key).unwrap();
    let rotation = level.selected().unwrap().transform.rotation.y;
    assert!((rotation - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    assert!((level.transform_value(TransformField::Rotation, 1).unwrap() - 90.0).abs() < 1e-4);

    // Re-entering the displayed value leaves authored precision alone.
    level.history(false);
    let edit = level.edit_selected(|entity| entity.transform.position.x = 1.234_567_8);
    assert!(edit.is_ok());
    let key = FieldKey::Transform(TransformField::Position, 0);
    level.drafts_mut().set_number(key.clone(), "1.2346", Default::default());
    level.commit_draft(&key).unwrap();
    assert_eq!(level.selected().unwrap().transform.position.x, 1.234_567_8);

    // Invalid numbers stay as drafts with an error and block saving and selection changes.
    let key = FieldKey::Transform(TransformField::Scale, 2);
    level.drafts_mut().set_number(key.clone(), "abc", Default::default());
    assert!(level.capture(false).is_err());
    assert!(level.drafts().get(&key).unwrap().error.is_some());
    assert!(!level.select(None), "invalid input blocks changing selection");
    assert_eq!(level.document().selected_id(), Some("box"));
    assert!(level.capture(true).is_err(), "polling never commits pending input");
    level.drafts_mut().set_number(key.clone(), "3", Default::default());
    let text = level.capture(false).unwrap();
    assert_eq!(parse_level(&text).unwrap().entities[0].transform.scale.z, 3.0);
}

#[test]
fn advanced_source_commits_only_valid_component_records() {
    let mut level = editor();
    level.select(Some("box"));
    let valid = level.components_source.clone();
    level.components_source = "{in-progress component source".to_owned();
    level.drafts_mut().set(FieldKey::LevelName, "Rename while editing components");
    assert!(level.commit_draft(&FieldKey::LevelName).is_err(), "in-progress source blocks other edits");
    assert_eq!(level.components_source, "{in-progress component source");
    assert!(level.capture(false).is_err());
    let mut components: Value = joy_engine::serde_json::from_str(&valid).unwrap();
    components["custom"] = json!({"nested": {"author": "preserved"}, "values": [1, true, "yes"]});
    level.components_source = joy_engine::serde_json::to_string_pretty(&components).unwrap();
    level.apply_components().unwrap();
    level.commit_draft(&FieldKey::LevelName).unwrap();
    let saved = parse_level(&level.capture(false).unwrap()).unwrap();
    assert_eq!(saved.name, "Rename while editing components");
    assert_eq!(Value::Object(saved.entities[0].components["custom"].clone()), components["custom"]);
}

#[test]
fn schema_driven_properties_add_fields_dim_inactive_fields_and_show_issues() {
    let mut level = editor();
    level.select(Some("box"));
    let assets = [AssetEntry { path: "assets/kart.form".into(), kind: "form".into() }];
    level.add_component(&ComponentChoice::Preset("physics.sphere".into()), None, &assets);
    level.add_component(&ComponentChoice::Preset("light.point".into()), None, &assets);
    level.change_property(&path(&["light", "range"]), Some(json!(0))).unwrap();
    level.add_component(&ComponentChoice::Preset("render.mesh".into()), None, &assets);
    assert!(level.status().contains("already has a render component"));
    level.add_component(&ComponentChoice::Preset("vehicle.four-wheel".into()), None, &assets);
    level.add_component(&ComponentChoice::Custom, Some("gameplay"), &assets);

    let components = level.display_components();
    let issues = level.selected_issues();
    let metadata = HashMap::new();
    let schemas = engine_component_catalog();
    let context = PropertyContext { schemas: &schemas, issues: &issues, metadata: &metadata, assets: &assets };
    let tree = build_property_tree(&components, &context);
    let group = |name: &str| {
        tree.iter()
            .find_map(|node| match node {
                PropertyNode::Group(group) if group.path == [name] => Some(group.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no {name} group"))
    };

    let physics = group("physics");
    let half_extents = physics.children.iter().find_map(|node| match node {
        PropertyNode::Vector(vector) if vector.path.last().map(String::as_str) == Some("halfExtents") => Some(vector),
        _ => None,
    });
    if let Some(vector) = half_extents {
        assert_eq!(vector.inactive.as_deref(), Some("Unused while shape is sphere."));
    } else {
        assert!(
            physics.missing_fields.iter().all(|(name, _)| name != "halfExtents"),
            "inactive fields are not suggested"
        );
    }
    let light = group("light");
    assert!(light.issues.iter().any(|issue| issue.contains("Range")), "{:?}", light.issues);
    let range = light.children.iter().find_map(|node| match node {
        PropertyNode::Field(field) if field.path == path(&["light", "range"]) => Some(field),
        _ => None,
    });
    assert!(range.is_some_and(|field| field.issue.is_some() && field.label.contains("· m")));
    assert_eq!(light.label, "Point Light");
    assert_eq!(light.type_name.as_deref(), Some("light"));
    let vehicle = group("vehicle");
    assert!(
        !vehicle.issues.iter().any(|issue| issue.contains("requires")),
        "the collider satisfies the vehicle: {:?}",
        vehicle.issues
    );
    let render = group("render");
    assert!(!render.missing_fields.is_empty(), "+ FIELD offers omitted schema fields");
    let custom = group("custom");
    assert!(custom.description.is_none() && custom.type_name.is_none(), "custom components keep generic controls");
    let gameplay = group("gameplay");
    assert!(gameplay.children.is_empty() && !gameplay.list);

    // Generic edits on custom data: new fields, list items, and removal.
    let record = Value::Object(components["custom"].as_object().unwrap().clone());
    let added = add_custom_field(&record, "speed", CustomFieldKind::Number).unwrap();
    assert!(add_custom_field(&added, "speed", CustomFieldKind::Text).is_err());
    level.change_property(&path(&["custom"]), Some(added)).unwrap();
    let list = level.display_components()["custom"]["nested"].clone();
    level.change_property(&path(&["custom", "nested"]), append_list_item(&list)).unwrap();
    assert_eq!(level.display_components()["custom"]["nested"].as_array().unwrap().len(), 3);
    level.change_property(&path(&["custom", "nested", "0"]), None).unwrap();
    assert_eq!(level.display_components()["custom"]["nested"][0]["label"], "retain me");
    level.change_property(&path(&["physics"]), None).unwrap();
    let issues = level.selected_issues();
    assert!(
        issues.iter().any(|issue| issue.message.contains("requires")),
        "a vehicle without physics reports it: {issues:?}"
    );
    level.change_property(&path(&["vehicle"]), None).unwrap();
    assert!(!level.display_components().contains_key("vehicle"));
}

#[test]
fn asset_selectors_keep_missing_references_and_expose_parameter_defaults() {
    let form = "project meta(name=\"Pillar\")\nparam height = 2 meta(min=0.5, max=5, step=0.1)\nbox(0, height*0.5, 0, 1, height, 1)\n";
    let mut access =
        MemoryAccess::default().with("assets/pillar.form", form).with("assets/spark.joyfx", &spark_effect());
    let mut level = editor();
    level.select(Some("box"));
    level.change_property(&path(&["render"]), Some(json!({"asset": "assets/pillar.form"}))).unwrap();
    let catalog = access.files.clone();
    level.add_component(&ComponentChoice::Preset("effect.particles".into()), None, &catalog);
    assert_eq!(
        level.display_components()["effect"]["asset"],
        "assets/spark.joyfx",
        "new asset fields take the first matching asset"
    );
    level.sync(&mut access);
    let components = level.display_components();
    assert_eq!(components["render"]["parameters"]["height"], json!(2), "FORM parameter defaults are shown");
    assert_eq!(components["effect"]["parameters"]["power"], json!(1.5), "JOYFX parameter defaults are shown");
    let bounds = &level.parameter_metadata()["render.parameters.height"];
    assert_eq!((bounds.min, bounds.max), (Some(0.5), Some(5.0)));
    let stored = parse_level(&level.capture(false).unwrap()).unwrap();
    assert!(stored.entities[0].components["render"].get("parameters").is_none(), "defaults are not stored");

    let schemas = engine_component_catalog();
    let mut components = Map::new();
    components.insert("render".into(), json!({"asset": "assets/gone.form"}));
    let issues = Vec::new();
    let metadata = HashMap::new();
    let context = PropertyContext { schemas: &schemas, issues: &issues, metadata: &metadata, assets: &catalog };
    let tree = build_property_tree(&components, &context);
    let PropertyNode::Group(render) = &tree[0] else { panic!("render group") };
    let PropertyNode::Field(asset) = &render.children[0] else { panic!("asset field") };
    let FieldControl::Asset(options) = &asset.control else { panic!("asset selector") };
    let values: Vec<&str> = options.iter().map(|(value, _)| value.as_str()).collect();
    assert_eq!(values, ["", "assets/gone.form", "assets/pillar.form"]);
    assert_eq!(options[1].1, "assets/gone.form (missing)");
}

#[test]
fn reusable_objects_link_instances_with_explicit_overrides() {
    let mut access = MemoryAccess::default();
    let mut level = editor();
    level.select(Some("box"));
    assert_eq!(level.suggested_object_path(), "box.joyobject");
    let path_written = level.save_object("crate.joyobject", &mut access).unwrap();
    assert_eq!(path_written, "assets/crate.joyobject");
    let object = parse_object(&access.texts["assets/crate.joyobject"]).unwrap();
    assert_eq!(object.entity.id, "object");
    assert_eq!(object.entity.transform.position, Vec3::ZERO);
    let instance = level.selected().unwrap().clone();
    assert_eq!(instance.template.as_deref(), Some("assets/crate.joyobject"));
    assert!(instance.components.is_empty());
    assert_eq!(level.resolve(&instance).components["render"]["shape"], "box");

    // Component edits become overrides; arrays override as a whole; nested removal needs the source.
    level.change_property(&path(&["render", "color", "1"]), Some(json!(0.75))).unwrap();
    let overrides = level.selected().unwrap().overrides.clone().unwrap();
    assert_eq!(overrides["components"]["render"]["color"], json!([0.9, 0.75, 0.1]));
    assert!(level.change_property(&path(&["render", "color"]), None).unwrap_err().contains("detach"));
    level.drafts_mut().set(FieldKey::EntityType, "prop");
    level.commit_draft(&FieldKey::EntityType).unwrap();
    assert!(
        level.selected().unwrap().overrides.as_ref().unwrap().get("type").is_none(),
        "unchanged values add no override"
    );
    level.reset_overrides();
    assert!(level.selected().unwrap().overrides.as_ref().unwrap().is_empty());

    level.add_instance("assets/crate.joyobject", &mut access).unwrap();
    assert_eq!(level.document().level().entities.len(), 2);
    level.detach();
    let detached = level.selected().unwrap();
    assert!(detached.template.is_none() && detached.components.contains_key("render"));
    level.open_source("assets/crate.joyobject");
    assert_eq!(level.take_open_requests(), ["assets/crate.joyobject"]);
}

#[test]
fn objects_edit_as_one_entity_and_save_as_objects() {
    let text = joy_editor::workspace::asset_types::create_asset_text("assets/crate.joyobject").unwrap();
    let mut level = LevelEditor::load("assets/crate.joyobject", &text).unwrap();
    assert!(level.object_mode());
    assert_eq!(level.document().selected_id(), Some("object"));
    assert_eq!(level.capture(false).unwrap(), text);
    level.drafts_mut().set(FieldKey::EntityName, "Sturdy crate");
    level.commit_draft(&FieldKey::EntityName).unwrap();
    assert_eq!(parse_object(&level.capture(false).unwrap()).unwrap().entity.name, "Sturdy crate");
}

#[test]
fn picking_tests_rotated_scaled_primitives_in_their_local_frames() {
    let entity = |id: &str, position: [f32; 3], components: Value| {
        let mut definition = EntityDefinition::from_json(&json!({"id": id, "components": components})).unwrap();
        definition.transform.position = Vec3::from_array(position);
        definition
    };
    let entities = [
        entity("near", [0.0, 0.0, 2.0], json!({"render": {"shape": "box"}})),
        entity("far", [0.0, 0.0, -2.0], json!({"render": {"shape": "sphere", "size": [2, 2, 2]}})),
        entity("effect", [5.0, 0.0, 0.0], json!({"effect": {"asset": "assets/fx.joyfx"}})),
        entity("hidden", [0.0, 5.0, 0.0], json!({"render": {"shape": "plane"}})),
    ];
    let ray = |origin: [f32; 3], direction: [f32; 3]| WorldRay {
        origin: Vec3::from_array(origin),
        direction: Vec3::from_array(direction).normalize(),
    };
    assert_eq!(pick_entity(&entities, |_| None, &ray([0.0, 0.0, 10.0], [0.0, 0.0, -1.0])).as_deref(), Some("near"));
    assert_eq!(pick_entity(&entities, |_| None, &ray([0.0, 0.0, -10.0], [0.0, 0.0, 1.0])).as_deref(), Some("far"));
    assert_eq!(pick_entity(&entities, |_| None, &ray([5.0, 10.0, 0.0], [0.0, -1.0, 0.0])).as_deref(), Some("effect"));
    assert_eq!(pick_entity(&entities, |_| None, &ray([0.0, 10.0, 0.0], [0.0, 1.0, 0.0])), None);
    // Mesh bounds replace the unit primitive.
    let mesh = [entity("mesh", [0.0, 0.0, 0.0], json!({"render": {"asset": "assets/kart.form"}}))];
    let bounds = |_: &str| Some((Vec3::new(0.0, 0.0, 0.0), Vec3::new(4.0, 1.0, 1.0)));
    assert_eq!(pick_entity(&mesh, bounds, &ray([3.5, 10.0, 0.5], [0.0, -1.0, 0.0])).as_deref(), Some("mesh"));
    assert_eq!(pick_entity(&mesh, |_| None, &ray([3.5, 10.0, 0.5], [0.0, -1.0, 0.0])), None);
}

#[test]
fn the_viewport_frames_its_camera_into_the_panel_region() {
    let mut viewport = LevelViewport::new();
    viewport.camera = OrbitCamera { distance: 10.0, ..Default::default() };
    viewport.viewport_size = Vec2::new(1600.0, 1000.0);
    viewport.region = ViewRegion { origin: Vec2::new(400.0, 100.0), size: Vec2::new(800.0, 600.0) };
    let target = viewport.project(viewport.camera.target).unwrap();
    assert!((target - viewport.region.center()).length() < 0.01, "{target}");
    // The region's own aspect keeps proportions: a point one unit right projects like on an 800 x 600 canvas.
    let canvas = viewport.camera.camera_frame(800.0 / 600.0);
    let right = viewport.camera.target + viewport.camera.basis().right;
    let on_canvas = project_to_viewport(right, canvas.view_projection(), viewport.region.size).unwrap();
    let in_region = viewport.project(right).unwrap();
    assert!((in_region - (viewport.region.origin + on_canvas)).length() < 0.05);
    // Screen rays through the region's center hit the camera target.
    let ray = viewport.screen_ray(viewport.region.center()).unwrap();
    let to_target = (viewport.camera.target - ray.origin).normalize();
    assert!(ray.direction.dot(to_target) > 0.9999);
}

/// A minimal valid effect with a scalar parameter.
pub fn spark_effect() -> String {
    json!({
        "version": 1, "name": "Spark", "maxParticles": 32,
        "parameters": {"power": {"value": 1.5, "min": 0, "max": 4}},
        "emitters": [{"id": "sparks", "enabled": true, "root": true, "duration": 1, "rate": 8, "bursts": [], "lifetime": 1,
            "renderer": "soft", "spawn": "size = 0.2 * power; color = rgba(1, 0.5, 0.1, 1);", "update": "color.a = 1 - t;"}]
    })
    .to_string()
}
