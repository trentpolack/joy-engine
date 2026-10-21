// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use super::*;
use serde_json::json;

#[test]
fn referenced_and_packed_instances_round_trip_and_resolve_their_scripts() {
    let text = r#"{"version":2,"name":"Tall tower","form":"../recipes/tower.form","overrides":{"floors":6.0,"lit":true,"sign":"OPEN"},"variants":[{"name":" Short ","overrides":{"floors":2}}]}"#;
    let instance = FormInstance::read(text).unwrap();
    assert_eq!(instance.script, InstanceScript::Reference("../recipes/tower.form".into()));
    assert_eq!(instance.serialize().unwrap(), text.replace("6.0", "6").replace(" Short ", "Short"));
    assert_eq!(
        instance.script_path("assets/form/city/tall.forminst").unwrap().as_deref(),
        Some("assets/form/recipes/tower.form")
    );
    assert_eq!(instance.overrides_for(Some("short")).unwrap()["floors"], json!(2));
    assert!(instance.overrides_for(Some("missing")).is_err());

    let modules = std::collections::HashMap::from([
        ("assets/form/recipes/tower.form", "use \"kit.form\" as kit\nparam floors = 3 meta(min=1, max=9, step=1)"),
        ("assets/form/recipes/kit.form", "let height = 3"),
    ]);
    let loader = |path: &str| modules.get(path).map(|text| text.to_string()).ok_or_else(|| "missing".to_owned());
    let script = instance.parse_script("assets/form/city/tall.forminst", &loader).unwrap();
    assert_eq!(script.module_paths(), vec!["assets/form/recipes/kit.form"]);

    let packed = FormInstance::packed("Packed", "param floors = 3 meta(min=1, max=9, step=1)");
    assert!(packed.parse_script("anywhere.forminst", &loader).is_ok());
    assert_eq!(FormInstance::read(&packed.serialize().unwrap()).unwrap(), packed);
}

#[test]
fn malformed_instances_are_rejected_with_stable_reasons() {
    let reject = |value: serde_json::Value| FormInstance::read(&value.to_string()).unwrap_err().to_string();
    assert_eq!(
        reject(json!({"version": 1, "name": "Old", "source": ""})),
        "This is not a valid FORM instance: it must be document version 2."
    );
    assert_eq!(
        reject(json!({"version": 2, "name": "Both", "source": "", "form": "a.form"})),
        "This is not a valid FORM instance: it needs either a \"form\" script path or packed \"source\", not both."
    );
    assert_eq!(
        reject(json!({"version": 2, "name": "Absolute", "form": "/etc/a.form"})),
        "This is not a valid FORM instance: the \"form\" path must be relative and use forward slashes."
    );
    assert_eq!(
        reject(json!({"version": 2, "name": "Text", "form": "a.txt"})),
        "This is not a valid FORM instance: the \"form\" path must name a .form script."
    );
    let escaping = FormInstance::referencing("Escape", "../../a.form");
    assert!(escaping.script_path("tall.forminst").is_err());
}
