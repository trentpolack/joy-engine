// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/scene_effects_controls.rs`.

use super::*;

#[test]
fn presets_and_toggles_build_typescript_configurations() {
    let mut controls = SceneEffectsControls::new();
    controls.select_preset(EffectPreset::Combined);
    let config = controls.configuration();
    assert!(config.gtao.is_some() && config.ssgi.is_some() && config.taa.is_some() && config.ssr.is_some());
    assert_eq!(config.ssgi.unwrap().ray_count, 7.0);
    assert_eq!(config.ssr.unwrap().resolution_scale, 0.5);
    assert!(controls.bloom);
    config.validate().unwrap();

    // Ambient-occlusion choices replace each other; manual changes become a custom selection.
    controls.toggle("ssao", true);
    assert!(controls.configuration().gtao.is_none() && controls.configuration().ssao.is_some());
    assert_eq!(controls.preset, EffectPreset::Custom);
    controls.quality = EffectQuality::High;
    assert_eq!(controls.configuration().ssao.unwrap().resolution_scale, 1.0);
    controls.select_preset(EffectPreset::Off);
    assert!(!controls.configuration().any_enabled() && !controls.bloom);
}

#[test]
fn tuning_supplies_enabled_effects_settings_per_quality() {
    fn tuning(quality: EffectQuality) -> SceneEffectsOptions {
        let scale = if quality == EffectQuality::High { 1.0 } else { 0.5 };
        SceneEffectsOptions {
            gtao: Some(GtaoOptions { radius: 2.2, resolution_scale: scale, ..Default::default() }),
            ..Default::default()
        }
    }
    let mut controls = SceneEffectsControls::new().with_tuning(tuning).with_bloom_strength(0.65);
    controls.select_preset(EffectPreset::Combined);
    let config = controls.configuration();
    assert_eq!(config.gtao.unwrap().radius, 2.2, "tuned entries replace the defaults");
    assert_eq!(config.ssgi.unwrap().ray_count, 7.0, "untuned entries keep the quality defaults");
    assert!(config.outlines.is_none(), "tuning never enables effects");
    controls.quality = EffectQuality::High;
    assert_eq!(controls.configuration().gtao.unwrap().resolution_scale, 1.0);
    assert_eq!(controls.bloom_strength, 0.65);
}
