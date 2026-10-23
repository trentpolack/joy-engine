// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Documents: reading, budgets, malformed input, every shipped asset at default and maximum parameters, typed
//! overrides, project renames, variants, and the gameplay examples' entity records.

use form_editor::compiler::{
    CompileRequest,
    evaluate,
    read_modules,
};
use form_editor::document::{
    DocumentError,
    FormDocument,
    read_document,
    read_overrides,
    read_variants,
    serialize_document,
};
use form_editor::form_assets::form_assets;
use form_editor::form_library::{
    FormLibraryAsset,
    library_reader,
    open_form_library_asset,
};
use form_editor::project_metadata::project_name_change;
use form_editor::script_guide::RECIPES;
use joy_engine::form::{
    CompiledScript,
    Diagnostic,
    GeometryData,
    default_material,
};
use joy_engine::serde_json::{
    self,
    Value as Json,
    json,
};

use crate::support::compile;

/// A document read back through JSON, as `readDocument(JSON.stringify(doc))`.
fn read(value: &Json) -> Result<FormDocument, DocumentError> {
    read_document(&value.to_string())
}

#[test]
fn documents_retain_source_name_and_numeric_overrides() {
    let source = "output(\"Slab\", box(1, 1, 1))";
    let document = json!({"version": 2, "name": "Study", "source": source, "overrides": {"size": 2}});
    let read = read(&document).unwrap();
    assert_eq!((read.name.as_str(), read.source.as_str()), ("Study", source));
    assert_eq!(read.to_json(), document);
}

#[test]
fn documents_round_trip_embedded_textures_beyond_the_former_one_megabyte_limit_and_enforce_the_budget() {
    let mut material = default_material();
    material["textures"]["baseColor"] =
        json!(format!("data:image/png;base64,{}", joy_engine::core::encode_base64(&vec![0; 800_000])));
    let document = json!({"version": 2, "name": "Textured", "source": "output(\"Slab\", box(1, 1, 1))", "overrides": {},
        "materials": [material]});
    let serialized = document.to_string();
    assert!(serialized.len() > 1_000_000);
    assert_eq!(read_document(&serialized).unwrap().to_json(), document);
    let error = read_document(&" ".repeat(64_000_001)).unwrap_err();
    assert!(error.to_string().contains("64 MB"), "{error}");
}

#[test]
fn rejects_unsupported_and_malformed_saved_documents() {
    for value in [
        json!(null),
        json!({}),
        json!({"version": 3, "name": "x", "source": ""}),
        json!({"version": 2, "name": "x", "source": "", "form": "x.form"}),
        json!({"version": 2, "name": "x", "source": "", "overrides": {"x": null}}),
    ] {
        assert!(read(&value).is_err(), "{value}");
    }
    assert!(read_document("{").is_err());
}

#[test]
fn all_shipped_form_assets_compile_at_default_and_maximum_parameter_settings() {
    for recipe in RECIPES {
        assert!(!compile(&recipe.code_for("study"), json!({}), None).positions.is_empty(), "{}", recipe.title);
    }
    let assets = form_assets();
    assert!(assets.len() >= 16, "the library finds the engine's examples and structures");
    for asset in &assets {
        let example = open_form_library_asset(asset, &assets).unwrap();
        let materials = example.materials.clone().map(Json::Array);
        let initial =
            compile_asset(asset, &example, Json::Object(example.overrides.clone()), materials.clone(), &assets);
        let mut overrides = example.overrides.clone();
        for parameter in &initial.parameters {
            let Some(maximum) = parameter.get("max").and_then(Json::as_f64) else {
                continue;
            };
            let value = match &parameter["value"] {
                Json::Array(components) => json!(vec![maximum; components.len()]),
                _ => json!(maximum),
            };
            overrides.insert(parameter["name"].as_str().unwrap().to_owned(), value);
        }
        let maximal = evaluate_asset(asset, &example, Json::Object(overrides), materials, &assets);
        assert!(maximal.is_ok(), "{}: {:?}", asset.path, maximal.err());
    }
}

#[test]
fn documents_round_trip_vector_and_color_overrides_and_reject_malformed_components() {
    let document = json!({"version": 2, "name": "Typed study", "source": "",
        "overrides": {"offset": [1, 2, 3], "tint": [1, 0.5, 0, 0.25]}});
    assert_eq!(read(&document).unwrap().to_json(), document);
    for value in [json!([1, 2]), json!([1, 2, 3, 4, 5]), json!([1, 2, null]), json!([1, 2, "3"])] {
        let mut invalid = document.clone();
        invalid["overrides"]["offset"] = value;
        assert!(read(&invalid).is_err());
    }
}

#[test]
fn project_name_edits_preserve_metadata_formatting_and_unrelated_script_text() {
    let source = "# name=\"untouched\"\nproject meta(info=\"A\", name=\"Old\", title=\"Keep me\")\noutput(\"Slab\", box(1, 1, 1))";
    let renamed = project_name_change(source, "New \"name\"").unwrap().apply(source);
    assert_eq!(
        renamed,
        "# name=\"untouched\"\nproject meta(info=\"A\", name=\"New \\\"name\\\"\", title=\"Keep me\")\noutput(\"Slab\", box(1, 1, 1))"
    );
    assert_eq!(compile(&renamed, json!({}), None).project.unwrap()["name"], "New \"name\"");
    let slab = "output(\"Slab\", box(1, 1, 1))";
    let inserted = project_name_change(slab, "A").unwrap();
    assert_eq!(compile(&format!("{}{slab}", inserted.insert), json!({}), None).project.unwrap()["name"], "A");
    assert!(project_name_change("box(", "A").is_err());
    // Columns count UTF-16 units, so names after wide characters still land on their quotes.
    let wide = "project meta(info=\"🌊🌊\", name=\"Old\")";
    assert_eq!(project_name_change(wide, "B").unwrap().apply(wide), "project meta(info=\"🌊🌊\", name=\"B\")");
    assert!(project_name_change("project meta(name=\"A\")\nproject meta(name=\"B\")", "C").is_err());
}

#[test]
fn named_variants_round_trip_owned_parameter_values_and_reject_ambiguous_or_oversized_collections() {
    let document = json!({"version": 2, "name": "Variants", "source": "", "overrides": {}, "variants": [
        {"name": "Tall", "overrides": {"height": 4, "tint": [1, 0.5, 0, 1]}},
        {"name": "Script defaults", "overrides": {}}
    ]});
    let mut restored = read(&document).unwrap();
    assert_eq!(restored.to_json(), document);
    restored.variants.as_mut().unwrap()[0].overrides.insert("tint".into(), json!([0, 0.5, 0, 1]));
    assert_eq!(read(&document).unwrap().variants.unwrap()[0].overrides["tint"], json!([1, 0.5, 0, 1]));
    let seventeen: Vec<Json> = (0..17).map(|index| json!({"name": index.to_string(), "overrides": {}})).collect();
    for variants in [
        json!(null),
        json!({}),
        json!([{"name": "", "overrides": {}}]),
        json!([document["variants"][0], document["variants"][0]]),
        json!([{"name": "Bad", "overrides": {"height": [4, 4]}}]),
        json!(seventeen),
        json!([{"name": " tall ", "overrides": {}}, {"name": "TALL", "overrides": {}}]),
    ] {
        let mut invalid = document.clone();
        invalid["variants"] = variants.clone();
        let error = read(&invalid).unwrap_err().to_string().to_lowercase();
        assert!(error.contains("variant") || error.contains("parameter"), "{variants}: {error}");
    }
}

#[test]
fn project_serialization_enforces_the_reopen_budget_across_all_variant_resources() {
    let mut material = default_material();
    material["textures"]["baseColor"] =
        json!(format!("data:image/png;base64,{}", joy_engine::core::encode_base64(&vec![0; 3_000_000])));
    let overrides = json!({"finish": {"kind": "material", "material": material}});
    let variants: Vec<Json> =
        (0..16).map(|index| json!({"name": format!("Take {index}"), "overrides": overrides})).collect();
    let heavy = FormDocument {
        name: "Heavy".into(),
        overrides: read_overrides(&overrides).unwrap(),
        variants: Some(read_variants(&json!(variants)).unwrap()),
        ..FormDocument::default()
    };
    let error = serialize_document(&heavy).unwrap_err();
    assert!(error.to_string().contains("64 MB"), "{error}");
    let mut small = heavy.clone();
    small.variants.as_mut().unwrap().truncate(1);
    assert_eq!(read_document(&serialize_document(&small).unwrap()).unwrap(), small);
}

/// One named output's snapshot record.
fn output(data: &GeometryData, name: &str) -> Json {
    data.outputs.as_ref().unwrap().iter().find(|output| output.name == name).unwrap().value.json()
}

/// Evaluate an opened library asset with the modules its script uses, as FORM EDITOR builds it: `use` paths resolve
/// beside the script itself, which for a referencing instance is the script it names.
fn evaluate_asset(
    asset: &FormLibraryAsset,
    document: &FormDocument,
    overrides: Json,
    materials: Option<Json>,
    assets: &[FormLibraryAsset],
) -> Result<GeometryData, Diagnostic> {
    let path = document.to_instance().script_path(&asset.path).unwrap().unwrap_or_else(|| asset.path.clone());
    let reader = library_reader(assets);
    let request = CompileRequest {
        source: document.source.clone(),
        overrides,
        materials,
        modules: read_modules(&document.source, &path, &|module| reader(module)),
        path: Some(path),
    };
    evaluate(&request).0
}

/// [`evaluate_asset`], panicking with the asset path and diagnostic on failure.
fn compile_asset(
    asset: &FormLibraryAsset,
    document: &FormDocument,
    overrides: Json,
    materials: Option<Json>,
    assets: &[FormLibraryAsset],
) -> GeometryData {
    evaluate_asset(asset, document, overrides, materials, assets)
        .unwrap_or_else(|error| panic!("{}: {error}", asset.path))
}

#[test]
fn gameplay_examples_expose_independent_entity_records_and_parameter_driven_generation() {
    let assets = form_assets();
    for (name, output_name, overrides, count, kind, attribute) in [
        ("harvest-grove", "Resource nodes", json!({"rows": 2, "columns": 3}), 6, "resource-node", "remaining"),
        ("patrol-circuit", "Waypoints", json!({"count": 5}), 5, "waypoint", "speed"),
        ("tactical-terraces", "Tiles", json!({"rows": 3, "columns": 4}), 12, "tile", "moveCost"),
    ] {
        let path = format!("assets/examples/{name}.form");
        let asset = assets.iter().find(|asset| asset.path == path).unwrap_or_else(|| panic!("{path}"));
        let program = CompiledScript::parse(&open_form_library_asset(asset, &assets).unwrap().source).unwrap();
        let result = program.evaluate(&overrides, None).unwrap();

        // Each entity's record fields are point attributes of the inspected points, one value per entity.
        let logical = output(&result, output_name);
        let points = &logical["attributes"]["points"];
        let column = |field: &str| -> Vec<Json> { points[field]["values"].as_array().unwrap().clone() };
        assert_eq!(column("position").len(), count);
        let ids: Vec<f64> = column("id").iter().map(|id| id.as_f64().unwrap()).collect();
        assert_eq!(ids, (0..count).map(|id| id as f64).collect::<Vec<_>>());
        assert_eq!(points["kind"]["type"], "text");
        assert!(column("kind").iter().all(|value| *value == kind));
        assert_eq!(column(attribute).len(), count);
        assert_eq!(program.evaluate(&overrides, None).unwrap(), result);
        match kind {
            "resource-node" => {
                let mut depleted_overrides = overrides.clone();
                depleted_overrides["harvestedFraction"] = json!(1);
                let depleted = program.evaluate(&depleted_overrides, None).unwrap();
                let nodes = output(&depleted, output_name);
                assert!(
                    nodes["attributes"]["points"]["remaining"]["values"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|value| value.as_f64() == Some(0.0))
                );
                // Harvesting changes only what remains; identities stay put.
                for field in ["id", "kind", "capacity"] {
                    assert_eq!(nodes["attributes"]["points"][field], points[field], "{field}");
                }
            }
            "waypoint" => {
                let next: Vec<f64> = column("nextId").iter().map(|next| next.as_f64().unwrap()).collect();
                assert_eq!(next, [1.0, 2.0, 3.0, 4.0, 0.0]);
            }
            _ => {
                assert_eq!(points["grid"]["size"], 3);
                assert_eq!(
                    joy_engine::core::integral_numbers_as_integers(column("grid")[11].clone()),
                    json!([3, 0, 2])
                );
                let mut dry_overrides = overrides.clone();
                dry_overrides["waterLevel"] = json!(0);
                let dry = program.evaluate(&dry_overrides, None).unwrap();
                let walkable = |data: &Json| -> Vec<bool> {
                    assert_eq!(data["attributes"]["points"]["walkable"]["type"], "bool");
                    data["attributes"]["points"]["walkable"]["values"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(Json::as_bool)
                        .collect()
                };
                let dry_tiles = walkable(&output(&dry, output_name));
                assert_eq!(dry_tiles.len(), count);
                assert!(dry_tiles.iter().all(|walkable| *walkable));
                assert!(walkable(&logical).contains(&false));
            }
        }

        // Results are owned: changing one never changes another evaluation.
        let baseline = serde_json::to_value(&result).unwrap();
        let mut changed = result.clone();
        let first = &mut changed.outputs.as_mut().unwrap()[0];
        first.name = "changed".into();
        first.geometry.positions[0] += 1.0;
        changed.positions[0] += 1.0;
        assert_eq!(serde_json::to_value(program.evaluate(&overrides, None).unwrap()).unwrap(), baseline);
    }
}
