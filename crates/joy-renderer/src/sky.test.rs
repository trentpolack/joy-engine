// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/sky.rs`.

use super::*;

#[test]
fn partial_authored_records_keep_defaults_and_reject_invalid_values() {
    let options = SkyOptions::from_json(&serde_json::json!({"clouds": {"coverage": 0.9}})).unwrap();
    assert_eq!(options.clouds.coverage, 0.9);
    assert_eq!(options.clouds.thickness, 700.0);
    assert_eq!(options.atmosphere, AtmosphereOptions::default());

    let unknown = SkyOptions::from_json(&serde_json::json!({"weather": "rain"})).unwrap_err();
    assert!(unknown.to_string().contains("weather"), "{unknown}");
    let nested = SkyOptions::from_json(&serde_json::json!({"clouds": {"coverage": 2}})).unwrap_err();
    assert_eq!(nested.to_string(), "clouds.coverage must be finite and between 0 and 1.");
    let positive = SkyOptions {
        ambient_occlusion: AmbientOcclusionOptions {
            radius: 0.0,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(positive.validate().unwrap_err().to_string().contains("ambientOcclusion.radius"));
}
