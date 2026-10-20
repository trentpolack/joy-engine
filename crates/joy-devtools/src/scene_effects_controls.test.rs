// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/scene_effects_controls.rs`.

use super::*;

#[test]
fn presets_and_toggles_build_their_configurations() {
    let mut controls = SceneEffectsControls::new();
    controls.select_preset(EffectPreset::Combined);
    let config = controls.configuration();
    assert!(config.taa.is_some() && config.motion_blur.is_some());
    assert!(config.adaptive_exposure.is_some() && config.volumetrics.is_some());
    assert_eq!(config.motion_blur.unwrap().sample_count, 12);
    assert_eq!(config.volumetrics.unwrap().quality, VolumetricQuality::Medium);
    assert!(controls.bloom);
    config.validate().unwrap();

    // Manual changes become a custom selection, and quality picks each effect's cost.
    controls.toggle(VOLUMETRICS_NAME, false);
    assert!(controls.configuration().volumetrics.is_none());
    assert_eq!(controls.preset, EffectPreset::Custom);
    controls.quality = EffectQuality::High;
    assert_eq!(controls.configuration().motion_blur.unwrap().sample_count, 16);
    controls.select_preset(EffectPreset::Off);
    assert!(!controls.configuration().any_enabled() && !controls.bloom);
}

#[test]
fn tuning_supplies_enabled_effects_settings_per_quality() {
    fn tuning(quality: EffectQuality) -> SceneEffectsOptions {
        let density = if quality == EffectQuality::High { 0.05 } else { 0.03 };
        SceneEffectsOptions {
            volumetrics: Some(VolumetricOptions {
                density,
                ..Default::default()
            }),
            ..Default::default()
        }
    }
    let mut controls = SceneEffectsControls::new().with_tuning(tuning).with_bloom_strength(0.65);
    controls.select_preset(EffectPreset::Atmosphere);
    let config = controls.configuration();
    assert_eq!(config.volumetrics.unwrap().density, 0.03, "tuned entries replace the defaults");
    assert!(config.adaptive_exposure.is_some(), "untuned entries keep the defaults");
    assert!(config.taa.is_none(), "tuning never enables effects");
    controls.quality = EffectQuality::High;
    assert_eq!(controls.configuration().volumetrics.unwrap().density, 0.05);
    assert_eq!(controls.bloom_strength, 0.65);
}
