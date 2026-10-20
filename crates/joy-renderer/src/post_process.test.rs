// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/post_process.rs`.

use super::*;
use serde_json::json;

fn overrides(record: Value) -> PostProcessOverrides {
    PostProcessOverrides::from_record(record.as_object().unwrap()).unwrap()
}

#[test]
fn defaults_cover_every_control_with_its_default() {
    let defaults = PostProcessSettings::DEFAULT.to_record();
    for control in POST_PROCESS_CONTROLS {
        let value = defaults.get(control.key).unwrap_or_else(|| panic!("{} has a default", control.key));
        match control.default {
            PostProcessControlDefault::Number(expected) => {
                assert!((value.as_f64().unwrap() - expected).abs() < 1e-6, "{}", control.key)
            }
            PostProcessControlDefault::Choice(expected) => {
                assert_eq!(value.as_str(), Some(expected), "{}", control.key)
            }
        }
    }
    assert_eq!(defaults.len(), POST_PROCESS_CONTROLS.len());
}

#[test]
fn invalid_values_are_rejected_and_leave_overrides_unchanged() {
    let reject = |record: Value| PostProcessOverrides::from_record(record.as_object().unwrap()).unwrap_err();
    assert_eq!(
        reject(json!({"bloomKnee": 0})),
        PostProcessError::BelowMinimum { key: "bloomKnee", minimum: 0.0, exclusive: true }
    );
    assert_eq!(reject(json!({"filmicToe": 0.5})).to_string(), "Post-processing filmicToe must be at least 1.");
    assert_eq!(
        reject(json!({"vignetteStrength": 2})),
        PostProcessError::AboveMaximum {
            key: "vignetteStrength",
            maximum: 1.0
        }
    );
    assert_eq!(reject(json!({"exposure": "bright"})), PostProcessError::NotFinite("exposure"));
    assert_eq!(reject(json!({"filmGrain": 0.1})), PostProcessError::UnknownSetting("filmGrain".into()));
    assert!(matches!(reject(json!({"toneMapper": "reinhard"})), PostProcessError::InvalidChoice { .. }));

    let mut edited = overrides(json!({"exposure": 2}));
    assert!(edited.set("exposure", json!(-1)).is_err());
    assert_eq!(edited.get("exposure"), Some(&json!(2)));
}

#[test]
fn blending_moves_numbers_exposure_and_choices_toward_the_overrides() {
    let target = overrides(json!({"filmicContrast": 2, "exposure": 4, "toneMapper": "aces"}));
    let base = PostProcessSettings::DEFAULT;
    assert_eq!(base.blend_toward(&target, 0.0), base);
    let full = base.with_overrides(&target);
    assert_eq!((full.filmic_contrast, full.exposure, full.tone_mapper), (2.0, 4.0, ToneMapper::Aces));
    assert_eq!(full.bloom_scatter, base.bloom_scatter, "omitted settings are unchanged");

    // Exposure interpolates in stops, so half weight from 1 toward 4 is 2; choices switch at half weight.
    let half = base.blend_toward(&target, 0.5);
    assert_eq!((half.filmic_contrast, half.exposure, half.tone_mapper), (1.5, 2.0, ToneMapper::Aces));
    assert_eq!(base.blend_toward(&target, 0.49).tone_mapper, ToneMapper::Filmic);
}
