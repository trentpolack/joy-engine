// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The FORM BUILDING example (`examples/form-building`), ported from its Node tests and browser captures: presets and
//! extreme parameters evaluate to valid deterministic geometry, material overrides reach the PBR asset, failed
//! evaluations leave the previous building intact, baked records round trip, the FORM LAB documents resolve through
//! `FormAssetLibrary` to the same building, and every preset and baked mode render.
//!
//! The render test needs a GPU adapter (llvmpipe or SwiftShader is enough); without one it prints a note and returns.
//! Set `JOY_RENDER_CAPTURE_DIR` to keep PNG captures. Native only.
#![cfg(not(target_arch = "wasm32"))]

#[path = "../../examples/form-building/src/mod.rs"]
mod example;

use example::asset::{
    asset_from_evaluation,
    geometry_from_mesh_data,
    mesh_data_from_evaluation,
};
use example::building_example::{
    BAKED_COTTAGE,
    BuildingExample,
    BuildingMode,
    SOURCE,
};
use joy_engine::core::MeshAssetData;
use joy_engine::form::{
    CompiledScript,
    GeometryData,
};
use joy_engine::rendering::{
    FormAssetLibrary,
    Shading,
};
use joy_engine::{
    HeadlessOptions,
    HeadlessShell,
};
use serde_json::{
    Map,
    Value,
    json,
};

const PRESETS: &str = include_str!("../../examples/form-building/assets/presets.json");

fn presets() -> Map<String, Value> {
    serde_json::from_str(PRESETS).expect("presets are a JSON object")
}

fn overrides(value: Value) -> Value {
    assert!(value.is_object());
    value
}

fn assert_valid(geometry: &GeometryData, name: &str) {
    let count = geometry.positions.len() / 3;
    assert!(count > 0, "{name}: geometry");
    assert!(geometry.positions.iter().all(|value| value.is_finite()), "{name}: finite positions");
    assert!(geometry.triangles.iter().all(|index| (*index as usize) < count), "{name}: indices in range");
    assert_eq!(geometry.triangle_materials.len(), geometry.triangles.len() / 3, "{name}: one material per triangle");
    assert!(
        geometry.triangle_materials.iter().all(|index| (*index as usize) < geometry.materials.len()),
        "{name}: materials exist"
    );
}

#[test]
fn presets_extremes_and_overrides_evaluate_deterministically() {
    let program = CompiledScript::parse(SOURCE).expect("building source parses");
    let mut cases: Vec<(String, Value)> = presets().into_iter().collect();
    cases.extend([
        (
            "minimum".to_owned(),
            json!({"width":4,"depth":4,"height":3,"floors":1,"window_spacing":6,"door_spacing":12,"roof_rise":0.3}),
        ),
        (
            "maximum".to_owned(),
            json!({"width":30,"depth":30,"height":30,"floors":8,"window_spacing":1.5,"door_spacing":3,"roof_rise":5}),
        ),
        ("fractional".to_owned(), json!({"floors":2.75,"roof_shape":0.75})),
        (
            "compressed".to_owned(),
            json!({"width":4,"depth":4,"height":3,"floors":8,"window_spacing":1.5,"door_spacing":3}),
        ),
        ("clamped".to_owned(), json!({"width":-100,"depth":999,"floors":999})),
    ]);
    let mut sizes = Vec::new();
    for (name, values) in cases {
        let geometry = program.evaluate(&overrides(values.clone()), None).expect("preset evaluates");
        assert_valid(&geometry, &name);
        assert_eq!(geometry, program.evaluate(&values, None).unwrap(), "{name}: deterministic");
        sizes.push((name, geometry.positions.len() / 3, geometry.triangles.len() / 3));
    }
    // The TypeScript findings' vertex and triangle counts.
    assert_eq!(sizes[0], ("cottage".to_owned(), 198, 290));
    assert_eq!(sizes[1], ("warehouse".to_owned(), 232, 348));
    assert_eq!(sizes[2], ("apartments".to_owned(), 1480, 2220));
    assert_eq!((sizes[4].1, sizes[4].2), (9710, 14558), "maximum shell");

    let seeded = |seed: u32| program.evaluate(&json!({"random_seed": seed}), None).unwrap().colors;
    assert_ne!(seeded(42), seeded(43), "the seed changes window tint");

    // A material override reaches the baked PBR vertices.
    let finish = json!({"kind": "material", "material": {
        "name": "Finish", "baseColor": [0.1, 0.7, 0.3, 1], "metallic": 0.65, "roughness": 0.12, "emissive": [0, 0, 0],
        "alphaMode": "OPAQUE", "alphaCutoff": 0.5,
        "textures": {"baseColor": null, "normal": null, "emissive": null, "metallic": null, "roughness": null, "ao": null}
    }});
    let geometry = program.evaluate(&json!({"wall_finish": finish}), None).expect("material override evaluates");
    let material = geometry.form_materials().into_iter().find(|material| material.base_color[1] == 0.7).unwrap();
    assert_eq!((material.metallic, material.roughness), (0.65, 0.12));
    let asset = asset_from_evaluation(&geometry).unwrap();
    assert!(asset.data().vertices.iter().any(|vertex| vertex.surface[0] == 0.65 && vertex.surface[1] == 0.12));
}

#[test]
fn failed_evaluations_keep_the_previous_building() {
    let mut building = BuildingExample::new(BuildingMode::Editable).expect("example starts");
    let before = building.snapshot();
    let recipe = building.recipe();
    for bad in [json!({"wall_tint": [1, 2, 3]}), json!({"window_piece": 3}), json!({"floors": "many"})] {
        let Value::Object(values) = bad else { unreachable!() };
        let error = building.regenerate(values).expect_err("invalid overrides fail");
        assert!(error.to_string().contains("line"), "diagnostics keep their location: {error}");
    }
    assert_eq!(building.snapshot(), before, "the previous generation stays");
    assert_eq!(building.recipe(), recipe);
    for source in [
        "box(0,0,0,0,1,1)",
        "repeat 20000 as i { instance(meshBox(1,1,1)) }",
        "let x = nope(1)",
        "if(1 or 0) {point(0,0,0)}",
    ] {
        let failure = CompiledScript::parse(source).and_then(|program| program.evaluate(&json!({}), None));
        assert!(failure.is_err(), "{source} fails with a located diagnostic");
    }

    // A hundred replacements keep one instance and leave earlier generations unchanged.
    let first = building.baked_mesh().unwrap();
    let apartments = presets()["apartments"].as_object().unwrap().clone();
    for seed in 1..=100 {
        let mut values = apartments.clone();
        values.insert("random_seed".to_owned(), json!(seed));
        building.regenerate(values).unwrap();
        assert_eq!(building.snapshot()["instances"], 1);
    }
    assert_eq!(building.snapshot()["evaluations"], 101);
    building.set_preset("cottage").unwrap();
    assert_eq!(building.baked_mesh().unwrap(), first);
}

#[test]
fn baked_records_and_formlab_documents_reproduce_the_building() {
    let program = CompiledScript::parse(SOURCE).unwrap();
    let presets = presets();
    let cottage = program.evaluate(&presets["cottage"], None).unwrap();

    // The baked record round trips through JSON with its materials, assignments, and bounds.
    let record: MeshAssetData =
        serde_json::from_str(&serde_json::to_string(&mesh_data_from_evaluation(&cottage)).unwrap()).unwrap();
    let restored = geometry_from_mesh_data(&record);
    assert_eq!(restored.materials, cottage.materials);
    assert_eq!(restored.triangle_materials, cottage.triangle_materials);
    let original_bounds = asset_from_evaluation(&cottage).unwrap().bounds();
    assert_eq!(asset_from_evaluation(&restored).unwrap().bounds(), original_bounds);

    // The supplied bake matches a fresh evaluation.
    let supplied: MeshAssetData = serde_json::from_str(BAKED_COTTAGE).unwrap();
    assert_eq!(supplied.positions.len(), cottage.positions.len());
    assert_eq!(asset_from_evaluation(&geometry_from_mesh_data(&supplied)).unwrap().bounds(), original_bounds);

    // FORM LAB documents carry the same source and preset overrides.
    let mut library = FormAssetLibrary::new();
    for (name, text) in [
        ("cottage", include_str!("../../examples/form-building/assets/cottage.formlab")),
        ("warehouse", include_str!("../../examples/form-building/assets/warehouse.formlab")),
        ("apartments", include_str!("../../examples/form-building/assets/apartments.formlab")),
    ] {
        let asset = library.resolve(&format!("assets/{name}.formlab"), text, &Map::new(), Shading::Pbr).unwrap();
        let direct = asset_from_evaluation(&program.evaluate(&presets[name], None).unwrap()).unwrap();
        assert_eq!(asset.bounds(), direct.bounds(), "{name}.formlab");
        assert_eq!(asset.data().vertices.len(), direct.data().vertices.len(), "{name}.formlab");
    }
}

#[test]
fn presets_and_baked_mode_render() {
    let options = HeadlessOptions { width: 640, height: 400, ..Default::default() };
    let mut shell = match HeadlessShell::new(options) {
        Ok(shell) => shell,
        Err(error) => {
            eprintln!("skipping GPU test: {error}");
            return;
        }
    };
    let capture = |shell: &mut HeadlessShell, building: &mut BuildingExample, name: &str| -> Vec<u8> {
        shell.run_frames(building, 2).expect("frames render");
        if let Ok(directory) = std::env::var("JOY_RENDER_CAPTURE_DIR") {
            std::fs::create_dir_all(&directory).expect("capture directory");
            shell.save_png(&std::path::Path::new(&directory).join(format!("{name}.png"))).expect("png written");
        }
        shell.capture().expect("readback")
    };
    // Pixels that differ from the background (the top-left corner): the building's coverage of the frame.
    let coverage = |pixels: &[u8]| {
        let background = [pixels[0], pixels[1], pixels[2]];
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| (0..3).any(|channel| pixel[channel].abs_diff(background[channel]) > 12))
            .count()
    };

    assert_eq!(BuildingExample::launch_mode(), BuildingMode::Editable, "editable unless --mode=baked");
    let mut building = BuildingExample::new(BuildingMode::Editable).unwrap();
    shell.start(&mut building).unwrap();
    let mut editable_cottage = Vec::new();
    for preset in ["cottage", "warehouse", "apartments"] {
        building.set_preset(preset).unwrap();
        let pixels = capture(&mut shell, &mut building, &format!("example-form-building-{preset}"));
        assert!(coverage(&pixels) > 640 * 400 / 20, "{preset} fills part of the view ({} pixels)", coverage(&pixels));
        if preset == "cottage" {
            editable_cottage = pixels;
        }
    }

    let mut baked = BuildingExample::new(BuildingMode::Baked).unwrap();
    let pixels = capture(&mut shell, &mut baked, "example-form-building-baked");
    assert_eq!(baked.snapshot()["evaluations"], 0, "baked mode never evaluates FORM");
    let different = pixels.iter().zip(&editable_cottage).filter(|(a, b)| a.abs_diff(**b) > 8).count();
    assert!(
        different < pixels.len() / 200,
        "the baked cottage matches the evaluated one ({different} channels differ)"
    );
}
