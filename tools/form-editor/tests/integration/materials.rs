// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Material palettes and MaterialX, plus the recorded MaterialX reference text (`tests/fixtures/materials.mtlx`).

use form_editor::materialx::{
    export_materialx,
    import_materialx,
};
use joy_engine::form::{
    default_material,
    read_materials,
};
use joy_engine::serde_json::{
    Value as Json,
    json,
};

use crate::support::fixture;

fn copper() -> Json {
    json!({"name": "Copper", "baseColor": [0.8, 0.25, 0.1, 0.4], "metallic": 0.9, "roughness": 0.2,
        "emissive": [0.1, 0.05, 0], "alphaMode": "MASK", "alphaCutoff": 0.35,
        "textures": {"baseColor": null, "normal": null, "emissive": null, "metallic": null, "roughness": null, "ao": null}})
}

/// Palette records compared by value, independent of integer or float spellings.
fn same(left: &[Json], right: &[Json]) -> bool {
    let normalize = |values: &[Json]| joy_engine::core::integral_numbers_as_integers(Json::Array(values.to_vec()));
    normalize(left) == normalize(right)
}

fn import_error(xml: &str) -> String {
    import_materialx(xml).expect_err(xml).0
}

#[test]
fn material_defaults_and_validated_reads_return_owned_records_with_default_first() {
    let defaults = read_materials(None).unwrap();
    assert_eq!(defaults, vec![default_material()]);
    assert!(same(
        &defaults,
        &[json!({"name": "Default", "baseColor": [1, 1, 1, 1], "metallic": 0, "roughness": 1, "emissive": [0, 0, 0],
            "alphaMode": "OPAQUE", "alphaCutoff": 0.5, "textures": {"baseColor": null, "normal": null,
            "emissive": null, "metallic": null, "roughness": null, "ao": null}})]
    ));
    let materials = read_materials(Some(&json!([copper()]))).unwrap();
    assert!(same(&materials, &[default_material(), copper()]));
}

#[test]
fn material_validation_rejects_malformed_duplicate_and_excessive_definitions() {
    let error = |value: Json| read_materials(Some(&value)).unwrap_err().0;
    assert!(error(json!(null)).contains("array"));
    let mut renamed = copper();
    renamed["name"] = json!("not valid");
    assert!(error(json!([renamed])).contains("name"));
    assert!(error(json!([copper(), copper()])).contains("unique"));
    let mut metallic = copper();
    metallic["metallic"] = json!(2);
    assert!(error(json!([metallic])).contains("metallic"));
    let mut short = copper();
    short["baseColor"] = json!([1, 1, 1]);
    assert!(error(json!([short])).contains("baseColor"));
    let mut extra = copper();
    extra["extra"] = json!(true);
    assert!(error(json!([extra])).contains("unsupported"));
    let many: Vec<Json> = (0..128)
        .map(|index| {
            let mut material = copper();
            material["name"] = json!(format!("M{index}"));
            material
        })
        .collect();
    assert!(error(json!(many)).contains("128"));
}

#[test]
fn materialx_constant_gltf_pbr_materials_round_trip_all_supported_values() {
    let mut rough = default_material();
    rough["roughness"] = json!(0.75);
    let source = read_materials(Some(&json!([rough, copper()]))).unwrap();
    let xml = export_materialx(&source).unwrap();
    assert!(xml.contains("<gltf_pbr name=\"FormLabShader0\" type=\"surfaceshader\">"));
    assert!(xml.contains("name=\"base_color\" type=\"color3\""));
    assert!(xml.contains("name=\"alpha_mode\" type=\"integer\""));
    assert!(same(&import_materialx(&xml).unwrap(), &source));
}

#[test]
fn materialx_text_matches_the_recording() {
    let mut rough = default_material();
    rough["roughness"] = json!(0.75);
    let mut glass = crate::support::glass();
    glass["textures"] = default_material()["textures"].clone();
    let xml = export_materialx(&[rough, glass]).unwrap();
    assert_eq!(xml, String::from_utf8(fixture("materials.mtlx")).unwrap());
}

#[test]
fn texture_budgets_reject_oversized_images_and_aggregate_documents() {
    let texture = format!("data:image/png;base64,{}", joy_engine::core::encode_base64(&vec![0; 4_000_000]));
    let mut first = default_material();
    for field in ["baseColor", "normal", "emissive", "metallic", "roughness", "ao"] {
        first["textures"][field] = json!(texture);
    }
    let mut second = default_material();
    second["name"] = json!("Other");
    second["textures"]["baseColor"] = json!(texture);
    second["textures"]["normal"] = json!(texture);
    assert_eq!(read_materials(Some(&json!([first, second]))).unwrap().len(), 2);
    second["textures"]["emissive"] = json!("data:image/png;base64,AA==");
    assert!(read_materials(Some(&json!([first, second]))).unwrap_err().0.contains("32 MB"));
    let mut oversized = default_material();
    oversized["textures"]["baseColor"] =
        json!(format!("data:image/png;base64,{}", joy_engine::core::encode_base64(&vec![0; 4_000_001])));
    assert!(read_materials(Some(&json!([oversized]))).unwrap_err().0.contains("4 MB"));
}

#[test]
fn materialx_export_rejects_textures_instead_of_silently_discarding_them() {
    let mut material = default_material();
    material["textures"]["baseColor"] = json!("data:image/png;base64,AA==");
    let error = export_materialx(&[material]).unwrap_err().0;
    assert!(error.contains("textures") && error.contains(".forminst") && error.contains("GLB"), "{error}");
}

#[test]
fn materialx_import_rejects_graphs_textures_and_unsupported_inputs_explicitly() {
    let xml = export_materialx(&[default_material()]).unwrap();
    let connected = import_error(&xml.replacen("/>", " nodename=\"image1\"/>", 1)).to_lowercase();
    assert!(connected.contains("connection") || connected.contains("constant"), "{connected}");
    let coat =
        import_error(&xml.replace("</gltf_pbr>", "<input name=\"coat\" type=\"float\" value=\"1\"/></gltf_pbr>"));
    assert!(coat.contains("Unsupported") && coat.contains("coat"), "{coat}");
    let image = import_error(&xml.replace("</materialx>", "<image name=\"texture\" type=\"color3\"/></materialx>"));
    assert!(image.contains("Unsupported") && image.contains("image"), "{image}");
}

#[test]
fn materialx_export_generates_shader_names_outside_the_shared_material_namespace() {
    let named = |name: &str| {
        let mut material = default_material();
        material["name"] = json!(name);
        material
    };
    let materials = read_materials(Some(&json!([named("Foo"), named("Foo_shader")]))).unwrap();
    let xml = export_materialx(&materials).unwrap();
    let imported = import_materialx(&xml).unwrap();
    let names: Vec<&str> = imported.iter().map(|material| material["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Default", "Foo", "Foo_shader"]);
    assert!(!xml.contains("<gltf_pbr name=\"Foo_shader\""));
    let collided = export_materialx(&[named("FormLabShader0")]).unwrap();
    assert!(collided.contains("<gltf_pbr name=\"_FormLabShader0\""), "{collided}");
}

#[test]
fn materialx_import_returns_only_authored_definitions_while_preserving_an_authored_default() {
    let xml = export_materialx(&[copper()]).unwrap();
    // Remove the Default material and its shader.
    let without_default = {
        let start = xml.find("  <surfacematerial name=\"Default\"").unwrap();
        let end = xml[start..].find("</surfacematerial>\n").unwrap() + start + "</surfacematerial>\n".len();
        let trimmed = format!("{}{}", &xml[..start], &xml[end..]);
        let shader_start = trimmed.find("  <gltf_pbr name=").unwrap();
        let shader_end = trimmed[shader_start..].find("</gltf_pbr>\n").unwrap() + shader_start + "</gltf_pbr>\n".len();
        format!("{}{}", &trimmed[..shader_start], &trimmed[shader_end..])
    };
    assert!(same(&import_materialx(&without_default).unwrap(), &[copper()]));
    let mut custom = default_material();
    custom["roughness"] = json!(0.3);
    assert!(same(&import_materialx(&export_materialx(std::slice::from_ref(&custom)).unwrap()).unwrap(), &[custom]));
}

#[test]
fn materialx_import_rejects_unsafe_declarations_unknown_versions_and_lossy_attributes() {
    let xml = export_materialx(&[default_material()]).unwrap();
    let doctype = import_error(&format!("<!DOCTYPE materialx [<!ENTITY x \"bad\">]>{xml}"));
    assert!(doctype.contains("DOCTYPE"), "{doctype}");
    assert!(import_error(&xml.replace("version=\"1.39\"", "version=\"9.0\"")).contains("version"));
    let colorspace =
        import_error(&xml.replace("name=\"base_color\"", "colorspace=\"srgb_texture\" name=\"base_color\""));
    assert!(colorspace.contains("colorspace"), "{colorspace}");
    assert!(import_error(&xml.replace("name=\"metallic\"", "unit=\"meter\" name=\"metallic\"")).contains("unit"));
    let interface = import_error(&xml.replace("name=\"roughness\"", "interfacename=\"roughness\" name=\"roughness\""));
    assert!(interface.contains("interfacename") || interface.contains("connection"), "{interface}");
}

#[test]
fn materialx_import_rejects_missing_and_globally_duplicated_node_names() {
    let xml = export_materialx(&[default_material()]).unwrap();
    let start = xml.find("<gltf_pbr name=\"").unwrap();
    let end = xml[start + 16..].find('"').unwrap() + start + 16;
    let shader_name = &xml[start + 16..end];
    let nameless = format!("{}<gltf_pbr{}", &xml[..start], &xml[end + 1..]);
    assert!(import_error(&nameless).contains("name"));
    let duplicated =
        xml.replace("name=\"Default\" type=\"material\"", &format!("name=\"{shader_name}\" type=\"material\""));
    let error = import_error(&duplicated);
    assert!(error.contains("duplicated") || error.contains("unique"), "{error}");
}

#[test]
fn materialx_import_rejects_empty_and_malformed_numeric_values() {
    let xml = export_materialx(&[default_material()]).unwrap();
    for malformed in ["", "1oops", "0x10", "1 2", "1,,2"] {
        let error = import_error(&xml.replacen("value=\"0\"/>", &format!("value=\"{malformed}\"/>"), 1));
        assert!(error.contains("invalid"), "{malformed}: {error}");
    }
}

#[test]
fn materialx_import_rejects_unreferenced_shaders_and_significant_element_text() {
    let xml = export_materialx(&[default_material()]).unwrap();
    let start = xml.find("  <gltf_pbr").unwrap();
    let end = xml.find("  </gltf_pbr>\n").unwrap() + "  </gltf_pbr>\n".len();
    let shader = &xml[start..end];
    let name_start = shader.find("name=\"").unwrap() + 6;
    let name_end = shader[name_start..].find('"').unwrap() + name_start;
    let extra = format!("{}UnusedShader{}", &shader[..name_start], &shader[name_end..]);
    let error = import_error(&xml.replace("</materialx>", &format!("{extra}</materialx>")));
    assert!(error.contains("UnusedShader") && error.contains("unreferenced"), "{error}");
    let text = import_error(&xml.replace("<materialx version=\"1.39\">", "<materialx version=\"1.39\">lost"));
    assert!(text.contains("text"), "{text}");
    assert_eq!(import_error("<materialx version=\"1.39\"><broken></materialx>"), "MaterialX XML is malformed.");
}
