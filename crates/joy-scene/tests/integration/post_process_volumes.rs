// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Post-process volume resolution: unbounded and bounded coverage, blend distance, entity placement and scale,
//! priority order, live overrides, and atomic syncs.

use joy_core::math::Vec3;
use joy_gameplay::{
    LevelData,
    parse_level,
};
use joy_renderer::{
    PostProcessOverrides,
    PostProcessSettings,
    post_process::ToneMapper,
};
use joy_scene::{
    LevelSceneError,
    PostProcessVolumes,
};
use serde_json::{
    Value,
    json,
};

/// Exposure is compared after single-precision blending.
const EXPOSURE_TOLERANCE: f32 = 1e-5;

fn level(entities: Value) -> LevelData {
    parse_level(&json!({"version": 1, "name": "Volumes", "entities": entities}).to_string()).unwrap()
}

fn volume(id: &str, position: [f32; 3], scale: [f32; 3], component: Value) -> Value {
    json!({"id": id, "transform": {"position": position, "scale": scale}, "components": {"postProcess": component}})
}

/// A global look, a scaled box with a fade-in, and a disabled sphere covering the origin.
fn authored_level() -> LevelData {
    level(json!([
        volume("global", [0.0, 0.0, 0.0], [1.0, 1.0, 1.0], json!({"unbounded": true, "exposure": 2})),
        volume(
            "cave",
            [10.0, 0.0, 0.0],
            [2.0, 2.0, 2.0],
            json!({"priority": 1, "halfExtents": [1, 1, 1], "blendDistance": 1, "exposure": 4, "toneMapper": "aces"})
        ),
        volume("off", [0.0, 0.0, 0.0], [1.0, 1.0, 1.0], json!({"enabled": false, "shape": "sphere", "radius": 5, "exposure": 0.5})),
        {"id": "crate", "components": {}}
    ]))
}

fn assert_exposure(settings: &PostProcessSettings, expected: f32) {
    assert!((settings.exposure - expected).abs() < EXPOSURE_TOLERANCE, "{} != {expected}", settings.exposure);
}

#[test]
fn bounded_volumes_cover_their_scaled_shape_and_fade_in_over_the_blend_distance() {
    let mut volumes = PostProcessVolumes::from_level(&authored_level()).unwrap();
    assert_eq!(volumes.len(), 3);
    assert_eq!(volumes.global_volume(), Some("global"));

    // Outside the box only the global look applies; the disabled sphere never does.
    let outside = volumes.resolve(Vec3::ZERO);
    assert_exposure(&outside, 2.0);
    assert_eq!(outside.tone_mapper, ToneMapper::Agx);

    // The entity's scale doubles the box to two meters a side; its center is fully inside.
    let inside = volumes.resolve(Vec3::new(10.0, 0.0, 0.0));
    assert_exposure(&inside, 4.0);
    assert_eq!(inside.tone_mapper, ToneMapper::Aces);

    // Half a meter inside the surface is half coverage: exposure blends in log space and choices switch at half.
    let edge = volumes.resolve(Vec3::new(11.5, 0.0, 0.0));
    assert_exposure(&edge, 2.0_f32 * std::f32::consts::SQRT_2);
    assert_eq!(edge.tone_mapper, ToneMapper::Aces);
    let contributions = volumes.contributions(Vec3::new(11.5, 0.0, 0.0));
    assert_eq!(contributions, [("global", 1.0), ("off", 0.0), ("cave", 0.5)]);

    // Past the scaled surface the box contributes nothing.
    assert_exposure(&volumes.resolve(Vec3::new(12.5, 0.0, 0.0)), 2.0);
}

#[test]
fn priorities_order_application_independently_of_authored_order() {
    let level = level(json!([
        volume("late", [0.0, 0.0, 0.0], [1.0, 1.0, 1.0], json!({"unbounded": true, "priority": 5, "contrast": 1.5})),
        volume(
            "early",
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            json!({"unbounded": true, "contrast": 0.5, "saturation": 0.5})
        ),
        volume("tie", [0.0, 0.0, 0.0], [1.0, 1.0, 1.0], json!({"unbounded": true, "saturation": 2, "weight": 0.5}))
    ]));
    let mut volumes = PostProcessVolumes::from_level(&level).unwrap();
    let settings = volumes.resolve(Vec3::ZERO);

    // The higher priority applies last even though it is authored first; equal priorities keep authored order.
    assert_eq!(settings.contrast, 1.5);
    assert_eq!(settings.saturation, 1.25);
    assert_eq!(volumes.global_volume(), Some("early"));
}

#[test]
fn live_overrides_replace_cached_results_and_failed_syncs_keep_the_previous_volumes() {
    let mut volumes = PostProcessVolumes::from_level(&authored_level()).unwrap();
    assert_exposure(&volumes.resolve(Vec3::ZERO), 2.0);

    let mut edited = PostProcessOverrides::new();
    edited.set("exposure", json!(3)).unwrap();
    assert!(volumes.set_overrides("global", edited));
    assert!(!volumes.set_overrides("crate", PostProcessOverrides::new()), "the crate has no volume");
    assert_exposure(&volumes.resolve(Vec3::ZERO), 3.0);

    let invalid = level(json!([volume("broken", [0.0, 0.0, 0.0], [1.0, 1.0, 1.0], json!({"exposure": "bright"}))]));
    match volumes.sync(&invalid.entities) {
        Err(LevelSceneError::Component { entity, .. }) => assert_eq!(entity, "broken"),
        other => panic!("expected a component error, got {other:?}"),
    }
    assert_eq!(volumes.len(), 3);
    assert_exposure(&volumes.resolve(Vec3::ZERO), 3.0);

    // A level without volumes resolves to the engine defaults.
    let mut empty = PostProcessVolumes::from_level(&level(json!([]))).unwrap();
    assert_eq!(empty.resolve(Vec3::ZERO), PostProcessSettings::DEFAULT);
}
