// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Effect documents: portable reads, recovery reads, size and version limits, detached examples, script whitespace, and
//! vector parameters; plus document edits and property snapshots.

use joy_engine::particles::{
    ParticleEffect,
    ParticleEffectOptions,
    ParticleParameter,
    ParticleParameterValue,
    compile_particle_effect,
};
use joy_engine::serde_json::{
    self,
    json,
};
use particle_lab::document::{
    self,
    Bound,
    ChildLink,
    MAX_ASSET_BYTES,
    ParameterKind,
    check_bound,
    property_snapshot,
    read_asset,
    read_recovery_asset,
    restore_properties,
    write_asset,
};
use particle_lab::examples::copy_example;

#[test]
fn authored_assets_survive_a_fully_validated_portable_json_round_trip() {
    let asset = copy_example(0);
    assert_eq!(read_asset(&write_asset(&asset)).unwrap(), asset);
    let text = write_asset(&asset);
    assert!(text.ends_with("}\n") && text.contains("\n  \"name\": \"Ember Impact\""), "two-space JSON with a newline");
    assert!(text.contains("\"maxParticles\": 2200"), "integral numbers stay integers: {}", &text[..200]);
}

#[test]
fn browser_recovery_preserves_an_unfinished_script_that_portable_import_rejects() {
    let mut asset = copy_example(0);
    asset.emitters[0].spawn = "size = (".to_owned();
    let text = write_asset(&asset);
    assert!(read_asset(&text).is_err());
    assert_eq!(read_recovery_asset(&text).unwrap(), asset);
    assert_eq!(read_recovery_asset("[]"), Err("Particle recovery must contain an effect object.".to_owned()));
}

#[test]
fn read_asset_delegates_malformed_nested_records_to_shared_validation() {
    let mut value = copy_example(0).to_json();
    value["emitters"][0] = serde_json::Value::Null;
    let message = read_asset(&value.to_string()).unwrap_err().to_lowercase();
    assert!(message.contains("emitter") || message.contains("object"), "{message}");
}

#[test]
fn read_asset_rejects_wrong_versions_and_bounded_size_violations() {
    let version = read_asset("{\"version\":2}").unwrap_err();
    assert!(version.to_lowercase().contains("version") && version.contains('1'), "{version}");
    let oversized = " ".repeat(MAX_ASSET_BYTES + 1);
    assert!(read_asset(&oversized).unwrap_err().contains("cannot exceed"));
    assert!(read_recovery_asset(&oversized).unwrap_err().contains("cannot exceed"));
}

#[test]
fn copy_example_detaches_mutable_child_records() {
    let mut first = copy_example(0);
    first.emitters[0].bursts[0].count = 1;
    assert_ne!(copy_example(0).emitters[0].bursts[0].count, 1);
}

#[test]
fn opening_an_asset_preserves_authored_script_whitespace_exactly() {
    let mut asset = copy_example(0);
    asset.emitters[0].spawn = "let speed = 4;\n\n  vx = speed; // intentional alignment\n".to_owned();
    let opened = read_asset(&write_asset(&asset)).unwrap();
    assert_eq!(opened.emitters[0].spawn, asset.emitters[0].spawn);
}

#[test]
fn vector_properties_round_trip_and_expose_xyz_script_values() {
    let mut asset = copy_example(0);
    asset.parameters.insert(
        "direction".to_owned(),
        ParticleParameter::Vector3 {
            value: [1.0, 2.0, 3.0],
            min: [-10.0; 3],
            max: [10.0; 3],
        },
    );
    asset.emitters[0].spawn =
        "vx = directionX; vy = directionY; vz = directionZ; drag = 0; ax = 0; ay = 0; az = 0;".to_owned();
    asset.emitters[0].update.clear();
    asset.emitters[0].trail = None;
    asset.emitters[0].death = None;
    asset.emitters.truncate(1);
    let opened = read_asset(&write_asset(&asset)).unwrap();
    let definition = compile_particle_effect(&opened.to_json()).unwrap();
    let options = ParticleEffectOptions {
        parameters: vec![("direction".to_owned(), ParticleParameterValue::Vector3([4.0, 5.0, 6.0]))],
        ..Default::default()
    };
    let mut effect = ParticleEffect::new(&definition, options).unwrap();
    effect.update(0.2).unwrap();
    assert!(!effect.particles().is_empty());
    assert!(effect.particles().iter().all(|particle| particle.vx == 4.0 && particle.vy == 5.0 && particle.vz == 6.0));
}

#[test]
fn property_snapshots_cover_values_but_never_scripts_or_texture_bytes() {
    let mut asset = copy_example(0);
    asset.emitters[0].texture = Some(document::default_texture(joy_engine::particles::DEFAULT_PARTICLE_TEXTURE));
    let before = property_snapshot(&asset);
    asset.emitters[0].spawn.push_str("\n// a newer script");
    assert_eq!(property_snapshot(&asset), before, "scripts are outside property undo");
    let ParticleParameter::Scalar {
        value,
        ..
    } = asset.parameters.values_mut().next().unwrap()
    else {
        panic!("Ember Impact starts with the scalar power");
    };
    *value += 0.25;
    let after = property_snapshot(&asset);
    assert_ne!(after, before);
    restore_properties(&mut asset, &before);
    assert_eq!(property_snapshot(&asset), before);
    assert!(asset.emitters[0].spawn.ends_with("// a newer script"), "restoring keeps the current scripts");
    let textured = asset.emitters.iter().find(|emitter| emitter.texture.is_some()).unwrap();
    assert!(textured.texture.as_ref().unwrap().source.starts_with("data:image/png;base64,"), "texture bytes are kept");
}

#[test]
fn document_edits_keep_references_and_refuse_breaking_changes() {
    let mut asset = copy_example(0);
    // Parameters used by scripts cannot be deleted, renamed, or retyped.
    assert_eq!(
        document::delete_parameter(&mut asset, "power"),
        Err("Parameter 'power' is used by a script and cannot be deleted.".to_owned())
    );
    assert!(document::rename_parameter(&mut asset, "wind", "breeze").unwrap_err().contains("cannot be renamed"));
    assert!(
        document::change_parameter_kind(&mut asset, "tint", ParameterKind::Scalar)
            .unwrap_err()
            .contains("changing type")
    );
    assert!(document::rename_parameter(&mut asset, "power", "wind").unwrap_err().contains("already exists"));
    let added = document::add_parameter(&mut asset);
    assert_eq!(added, "parameter1");
    assert_eq!(document::rename_parameter(&mut asset, &added, " gust ").unwrap(), Some("gust".to_owned()));
    document::change_parameter_kind(&mut asset, "gust", ParameterKind::Vector).unwrap();
    assert!(matches!(asset.parameters["gust"], ParticleParameter::Vector3 { .. }));
    document::delete_parameter(&mut asset, "gust").unwrap();

    // Bounds keep minimum no greater than maximum and clamp values.
    let tint = asset.parameters["tint"].clone();
    assert!(check_bound(&tint, Bound::Minimum, 0, 1.5).unwrap_err().contains("within 0–1"));
    assert!(check_bound(&tint, Bound::Maximum, 3, 0.5).is_ok());
    let power = asset.parameters["power"].clone();
    assert!(check_bound(&power, Bound::Minimum, 0, 2.5).is_err(), "minimum above maximum");

    // Emitters: renames follow child links, deletes drop them, and ids stay unique.
    document::rename_emitter(&mut asset, 3, "ash").unwrap();
    assert_eq!(asset.emitters[0].death.as_ref().unwrap().emitter, "ash");
    assert_eq!(asset.emitters[1].trail.as_ref().unwrap().emitter, "ash");
    assert!(document::rename_emitter(&mut asset, 0, "sparks").unwrap_err().contains("already exists"));
    let copy = document::duplicate_emitter(&mut asset, 1).unwrap();
    assert_eq!(asset.emitters[copy].id, "sparks_copy");
    assert_eq!(document::unique_emitter_id(&asset, "sparks copy"), "sparks_copy_2");
    assert_eq!(document::delete_emitter(&mut asset, 3).unwrap(), 2);
    assert!(asset.emitters.iter().all(|emitter| emitter.trail.is_none() && emitter.death.is_none()));
    document::set_child_target(&mut asset.emitters[0], ChildLink::Trail, Some("billow"));
    assert_eq!(asset.emitters[0].trail.as_ref().map(|trail| (trail.count, trail.interval)), Some((1, 0.1)));
    let mut single = copy_example(0);
    single.emitters.truncate(1);
    assert_eq!(document::delete_emitter(&mut single, 0), Err("An asset needs at least one emitter.".to_owned()));
    let _ = json!({});
}
