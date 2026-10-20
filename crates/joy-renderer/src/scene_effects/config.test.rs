// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/scene_effects/config.rs`.

use super::*;

#[test]
fn defaults_validate_and_report_their_names_and_needs() {
    let all = SceneEffectsOptions {
        taa: Some(TaaOptions::default()),
        motion_blur: Some(MotionBlurOptions::default()),
        adaptive_exposure: Some(AdaptiveExposureOptions::default()),
        volumetrics: Some(VolumetricOptions::default()),
    };
    all.validate().unwrap();
    assert_eq!(all.enabled_names().collect::<Vec<_>>(), SCENE_EFFECT_NAMES);
    assert!(all.needs_motion_vectors());

    let exposure_only = SceneEffectsOptions {
        adaptive_exposure: Some(Default::default()),
        ..Default::default()
    };
    assert!(!exposure_only.needs_motion_vectors() && exposure_only.any_enabled());
    assert!(!SceneEffectsOptions::default().any_enabled());
}

#[test]
fn out_of_range_settings_are_rejected_by_name() {
    let reject = |options: SceneEffectsOptions| options.validate().unwrap_err().to_string();
    let taa = SceneEffectsOptions {
        taa: Some(TaaOptions {
            history_weight: 1.0,
        }),
        ..Default::default()
    };
    assert_eq!(reject(taa), "Scene effect taa.historyWeight must be in [0, 0.98].");

    let blur = MotionBlurOptions {
        sample_count: 64,
        ..Default::default()
    };
    assert!(
        reject(SceneEffectsOptions {
            motion_blur: Some(blur),
            ..Default::default()
        })
        .contains("sampleCount")
    );

    let exposure = AdaptiveExposureOptions {
        minimum_exposure: 4.0,
        maximum_exposure: 2.0,
        ..Default::default()
    };
    let message = reject(SceneEffectsOptions {
        adaptive_exposure: Some(exposure),
        ..Default::default()
    });
    assert!(message.contains("maximumExposure"), "{message}");

    let fog = VolumetricOptions {
        density: f32::NAN,
        ..Default::default()
    };
    assert!(
        reject(SceneEffectsOptions {
            volumetrics: Some(fog),
            ..Default::default()
        })
        .contains("density")
    );
}
