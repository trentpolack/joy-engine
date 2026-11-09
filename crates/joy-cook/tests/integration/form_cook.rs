// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The cook end to end: real engine FORM assets and a scratch project with modules, instances, and variants cook into
//! a bundle whose geometry is exactly what live evaluation produces, and the cache re-evaluates only what changed.

use std::path::{
    Path,
    PathBuf,
};

use joy_cook::{
    CookCache,
    CookRequest,
    FormCook,
};
use joy_form::{
    CookedForms,
    FormEvaluation,
    GeometryData,
};
use serde_json::{
    Map,
    json,
};

/// A module and a script that uses it, the way FRAMEBREAK's city scripts share a building kit.
const KIT_SOURCE: &str = "fn slab(width) { return box(width, 0.5, 2) }\n";
const TOWER_SOURCE: &str =
    "use \"kit.form\" as kit\nparam floors = 3 meta(min=1, max=12, step=1)\noutput(\"Tower\", kit.slab(floors))\n";

/// An instance of the tower with its own overrides and two variants.
const TOWER_INSTANCE: &str = r#"{
  "version": 2,
  "name": "tall tower",
  "form": "tower.form",
  "overrides": {"floors": 5},
  "variants": [
    {"name": "Stub", "overrides": {"floors": 1}},
    {"name": "Spire", "overrides": {"floors": 9}}
  ]
}"#;

#[test]
fn cooked_meshes_match_live_evaluation() {
    let project = ScratchProject::new("round-trip");
    project.write("assets/form/kit.form", KIT_SOURCE);
    project.write("assets/form/tower.form", TOWER_SOURCE);
    project.write("assets/form/tower.forminst", TOWER_INSTANCE);

    // A scratch project (modules, an instance, and its variants) and the engine's own building assets.
    let engine_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut scratch = FormCook::new(project.root());
    scratch.add_folder("assets/form", &["assets/form/kit.form"]).unwrap();
    let mut engine = FormCook::new(&engine_root);
    engine.add_folder("assets/form/structures", &[]).unwrap();

    for (root, cook) in [(project.root().to_path_buf(), scratch), (engine_root, engine)] {
        let report = cook.cook().unwrap();
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        let cooked = CookedForms::from_bytes(&report.bundle).unwrap();
        for entry in &report.cooked {
            // Resolve the request the way a level does at run time; the key must find the cooked copy.
            let request = &entry.request;
            let text = std::fs::read_to_string(root.join(&request.path)).unwrap();
            let loader = |module: &str| std::fs::read_to_string(root.join(module)).map_err(|error| error.to_string());
            let evaluation =
                FormEvaluation::resolve(&request.path, &text, &loader, request.variant.as_deref(), &request.parameters)
                    .unwrap();
            assert_eq!(evaluation.key(), entry.key, "{}", request.label());

            let live = evaluation.evaluate().unwrap();
            let loaded = cooked.geometry(entry.key).expect("cooked").unwrap();
            assert_eq!(
                loaded,
                GeometryData {
                    outputs: None,
                    ..live.clone()
                },
                "{}",
                request.label()
            );
            assert_eq!(loaded.to_mesh(), live.to_mesh(), "{}", request.label());
            assert_eq!(loaded.form_materials(), live.form_materials(), "{}", request.label());
        }
    }

    // The scratch project cooked the script, the instance, and both variants; the module alone was skipped by name.
    let mut cook = FormCook::new(project.root());
    cook.add_folder("assets/form", &["assets/form/kit.form"]).unwrap();
    let labels: Vec<String> = cook.requests().iter().map(CookRequest::label).collect();
    assert_eq!(
        labels,
        [
            "assets/form/tower.form",
            "assets/form/tower.forminst",
            "assets/form/tower.forminst [Stub]",
            "assets/form/tower.forminst [Spire]"
        ]
    );
}

#[test]
fn the_cache_reuses_records_until_a_source_module_or_parameter_changes() {
    let project = ScratchProject::new("cache");
    project.write("assets/form/kit.form", KIT_SOURCE);
    project.write("assets/form/tower.form", TOWER_SOURCE);
    let cache = CookCache::new(project.root().join("cache"));
    let cook = |parameters: Map<String, serde_json::Value>| {
        let mut cook = FormCook::new(project.root()).with_cache(cache.clone());
        cook.add(CookRequest {
            parameters,
            ..CookRequest::defaults("assets/form/tower.form")
        });
        let report = cook.cook().unwrap();
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        (report.cooked[0].key, report.cooked[0].from_cache)
    };

    // The first cook evaluates; the second reads the cache.
    let (first, cached) = cook(Map::new());
    assert!(!cached);
    assert_eq!(cook(Map::new()), (first, true));

    // Editing the module, the script, or the parameters evaluates again under a new key.
    project.write("assets/form/kit.form", "fn slab(width) { return box(width, 0.75, 2) }\n");
    let (module_edit, cached) = cook(Map::new());
    assert!(!cached && module_edit != first);
    project.write("assets/form/tower.form", &TOWER_SOURCE.replace("floors = 3", "floors = 4"));
    let (source_edit, cached) = cook(Map::new());
    assert!(!cached && source_edit != module_edit);
    let (parameter_edit, cached) = cook(json!({"floors": 7}).as_object().unwrap().clone());
    assert!(!cached && parameter_edit != source_edit);

    // A damaged cache file is ignored and rewritten rather than trusted.
    std::fs::write(cache.record_path(parameter_edit), b"JFGM damaged").unwrap();
    assert_eq!(cook(json!({"floors": 7}).as_object().unwrap().clone()), (parameter_edit, false));
    assert_eq!(cook(json!({"floors": 7}).as_object().unwrap().clone()), (parameter_edit, true));
}

/// A project folder under the system temporary directory, removed when the test ends.
struct ScratchProject {
    root: PathBuf,
}

impl ScratchProject {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("joy-cook-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self {
            root,
        }
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn write(&self, path: &str, text: &str) {
        let file = self.root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
}

impl Drop for ScratchProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
