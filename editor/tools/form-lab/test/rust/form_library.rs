// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The Examples library, ported from `test/form-library.test.js`, plus the bundled copy browsers use.

use form_lab::form_assets::{bundled_form_assets, create_initial_form_document, form_assets};
use form_lab::form_library::{create_form_library, open_form_library_asset};
use joy_engine::form::default_material;
use joy_engine::serde_json::json;

#[test]
fn the_form_library_opens_independent_script_and_document_copies_only_from_its_authored_asset_roots() {
    let document = json!({"version": 1, "name": "Stored take", "source": "point(0,0,0)", "overrides": {"height": 3.5},
        "materials": [default_material()], "variants": [{"name": "Tall", "overrides": {"height": 6}}]});
    let files = [
        ("assets/form/structures/study.formlab", document.to_string()),
        ("assets/examples/nested/recipe.form", "# Authored source\npoint(1,2,3)\n".to_owned()),
        ("assets/form/ignored.txt", "Not a FORM asset.".to_owned()),
        ("assets/meshes/ignored.form", "Outside the requested library.".to_owned()),
        ("assets/form/../private.form", "Not a library path.".to_owned()),
    ];
    let assets = create_form_library(files.iter().map(|(path, text)| ((*path).to_owned(), text.clone())));
    let paths: Vec<&str> = assets.iter().map(|asset| asset.path.as_str()).collect();
    assert_eq!(paths, ["assets/examples/nested/recipe.form", "assets/form/structures/study.formlab"]);
    assert_eq!(assets[0].folder, "assets/examples/nested");
    assert_eq!(assets[0].filename, "recipe.form");
    let script = open_form_library_asset(&assets[0]).unwrap();
    assert_eq!(script.source, "# Authored source\npoint(1,2,3)\n");
    assert_eq!(script.name, "recipe");
    let mut opened = open_form_library_asset(&assets[1]).unwrap();
    assert_eq!(opened.to_json(), document);
    opened.overrides.insert("height".into(), json!(9));
    opened.materials.as_mut().unwrap()[0]["baseColor"][0] = json!(0);
    opened.variants.as_mut().unwrap()[0].overrides.insert("height".into(), json!(10));
    assert_eq!(
        open_form_library_asset(&assets[1]).unwrap().to_json(),
        document,
        "opening a library asset again must not inherit authoring mutations"
    );
}

#[test]
fn bundled_assets_match_the_files_on_disk_and_open_the_wave_study_first() {
    let disk = form_assets();
    let bundled = bundled_form_assets();
    assert_eq!(disk, bundled, "the browser bundle carries every authored file");
    assert!(bundled.iter().any(|asset| asset.path == "assets/examples/vessel-workshop.form"));
    assert!(bundled.iter().any(|asset| asset.path == "assets/form/structures/cottage.formlab"));
    let initial = create_initial_form_document(&bundled);
    assert_eq!(initial.name, "wave study");
    assert!(create_initial_form_document(&[]).source.is_empty(), "an empty library still opens");
}
