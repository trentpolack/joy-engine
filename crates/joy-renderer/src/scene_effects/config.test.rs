// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/scene_effects/config.rs`.

use super::*;

#[test]
fn defaults_validate_and_report_requirements() {
    let all = SceneEffectsOptions {
        depth_aware_blur: Some(Default::default()),
        gtao: Some(Default::default()),
        ssgi: Some(Default::default()),
        outlines: Some(Default::default()),
        taa: Some(Default::default()),
        motion_blur: Some(Default::default()),
        ssr: Some(Default::default()),
        height_fog: Some(Default::default()),
        clustered_lighting: Some(Default::default()),
        adaptive_exposure: Some(Default::default()),
        ..Default::default()
    };
    all.validate().unwrap();
    let requirements = all.requirements();
    assert!(requirements.depth_texture && requirements.normal_texture && requirements.velocity_texture);
    assert!(requirements.taa_velocity_texture && requirements.clustered_lighting);
    assert_eq!(all.enabled_names().count(), 10);

    let exposure_only = SceneEffectsOptions { adaptive_exposure: Some(Default::default()), ..Default::default() };
    assert!(!exposure_only.requirements().depth_texture);
    assert!(!SceneEffectsOptions::default().any_enabled());
}

#[test]
fn invalid_settings_report_typescript_messages() {
    let both =
        SceneEffectsOptions { ssao: Some(Default::default()), gtao: Some(Default::default()), ..Default::default() };
    assert_eq!(both.validate(), Err(SceneEffectsError::ExclusiveOcclusion));
    let scale = SceneEffectsOptions {
        ssao: Some(SsaoOptions { resolution_scale: 0.1, ..Default::default() }),
        ..Default::default()
    };
    assert_eq!(
        scale.validate().unwrap_err().to_string(),
        "ssao.resolutionScale must be a finite number from 0.25 through 1."
    );
    let rays =
        SceneEffectsOptions { ssgi: Some(SsgiOptions { ray_count: 2.5, ..Default::default() }), ..Default::default() };
    assert_eq!(rays.validate(), Err(SceneEffectsError::NotAnInteger("ssgi.rayCount".into())));
    let exposure = SceneEffectsOptions {
        adaptive_exposure: Some(AdaptiveExposureOptions { minimum_exposure: 9.0, ..Default::default() }),
        ..Default::default()
    };
    assert_eq!(exposure.validate(), Err(SceneEffectsError::ExposureBounds));
}

#[test]
fn options_read_typescript_json_spellings() {
    let options: SceneEffectsOptions = serde_json::from_str(
        r#"{"ssao": {"normalSource": "normal-texture"}, "taa": {"reprojection": "camera"}, "colorFormat": "rgba8unorm"}"#,
    )
    .unwrap();
    assert_eq!(options.ssao.unwrap().normal_source, NormalSource::NormalTexture);
    assert_eq!(options.ssao.unwrap().radius, 7.0);
    assert_eq!(options.taa.unwrap().reprojection, Reprojection::Camera);
    assert_eq!(options.color_format, Some(SceneEffectsColorFormat::Rgba8unorm));
    assert!(serde_json::from_str::<SceneEffectsOptions>(r#"{"bloom": {}}"#).is_err());
}
