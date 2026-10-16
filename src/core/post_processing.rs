// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Display (post-processing) configuration: schema, defaults, presets, validation, and authored profiles.
//!
//! This is plain data shared by level documents, tools, and the renderer. It lives in the foundation layer so authored
//! levels can validate their profile without depending on rendering. Serialized names, defaults, ranges, presets, and
//! the version-one `blue-noise` migration match the TypeScript engine, so the same `.joylevel` profile resolves to the
//! same settings in both implementations.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::core::JsonRecord;

/// Only supported authored profile version.
pub const PROFILE_VERSION: u64 = 1;

/// Version-one spelling of decorrelated hash grain, migrated on load.
const LEGACY_BLUE_NOISE: &str = "blue-noise";

/// Display transform applied after linear HDR composition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToneMapper {
    /// AgX with optional punchy look (default).
    Agx,
    /// Fitted ACES filmic curve.
    Aces,
    /// Linear exposure with a display clamp.
    None,
}

/// AgX grading look.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgxLook {
    /// The base AgX curve.
    Neutral,
    /// Adds AgX's power and saturation look before the display transform.
    Punchy,
}

/// Bloom pyramid depth.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BloomQuality {
    /// Two pyramid levels.
    Low,
    /// Three pyramid levels.
    Medium,
    /// Four pyramid levels.
    High,
    /// Five pyramid levels.
    Ultra,
}

impl BloomQuality {
    /// Number of downsampled HDR levels used by the bloom pyramid.
    pub fn levels(self) -> usize {
        match self {
            Self::Low => 2,
            Self::Medium => 3,
            Self::High => 4,
            Self::Ultra => 5,
        }
    }
}

/// Film-grain hash variant. Neither variant claims a blue-noise spectrum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FilmGrainNoise {
    /// Single analog-style hash.
    Analog,
    /// Average of two offset hashes.
    Decorrelated,
}

/// Complete, validated display settings. Lengths are fractions of the shorter viewport axis unless noted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostProcessingConfig {
    /// Display transform after linear HDR composition.
    pub tone_mapper: ToneMapper,
    /// Edge-only scene smoothing in `[0, 1]`; zero disables it.
    pub antialias_strength: f32,
    /// Linear scene exposure before tone mapping.
    pub exposure: f32,
    /// Bright-pass glow strength; zero disables bloom.
    pub bloom_strength: f32,
    /// Blur reach as a fraction of the shorter viewport axis.
    pub bloom_radius: f32,
    /// Linear luminance at which glow begins.
    pub bloom_threshold: f32,
    /// Width of the soft transition into bloom; always positive.
    pub bloom_knee: f32,
    /// Horizontal stretch of the bloom halo in `[0, 1]`.
    pub bloom_anamorphic: f32,
    /// Bloom pyramid depth.
    pub bloom_quality: BloomQuality,
    /// Contribution of the wider bloom levels during reconstruction.
    pub bloom_scatter: f32,
    /// Horizontal diffraction streak strength.
    pub bloom_lens_flare: f32,
    /// Display brightness multiplier in `[0, 2]`.
    pub brightness: f32,
    /// Muted-color saturation adjustment in `[-1, 1]`.
    pub vibrance: f32,
    /// Full-image Gaussian blur radius as a fraction of the shorter axis.
    pub blur_radius: f32,
    /// Display saturation after tone mapping.
    pub saturation: f32,
    /// Display contrast around middle gray.
    pub contrast: f32,
    /// Luminance-aware grain intensity.
    pub film_grain: f32,
    /// Grain cell size in physical pixels.
    pub film_grain_size: f32,
    /// Grain animation rate in cycles per second.
    pub film_grain_speed: f32,
    /// Grain hash variant.
    pub film_grain_noise: FilmGrainNoise,
    /// Edge darkening strength.
    pub vignette_strength: f32,
    /// Normalized radius where edge darkening begins; always positive.
    pub vignette_radius: f32,
    /// Blur toward the vignette edge in `[0, 1]`.
    pub vignette_blur: f32,
    /// AgX grading look.
    pub agx_look: AgxLook,
}

impl Default for PostProcessingConfig {
    fn default() -> Self {
        Self {
            tone_mapper: ToneMapper::Agx,
            antialias_strength: 1.0,
            exposure: 1.0,
            bloom_strength: 0.0,
            bloom_radius: 0.025,
            bloom_threshold: 1.0,
            bloom_knee: 0.35,
            bloom_anamorphic: 0.0,
            bloom_quality: BloomQuality::High,
            bloom_scatter: 0.55,
            bloom_lens_flare: 0.0,
            brightness: 1.0,
            vibrance: 0.0,
            blur_radius: 0.0,
            saturation: 1.0,
            contrast: 1.0,
            film_grain: 0.0,
            film_grain_size: 1.5,
            film_grain_speed: 18.0,
            film_grain_noise: FilmGrainNoise::Decorrelated,
            vignette_strength: 0.0,
            vignette_radius: 0.72,
            vignette_blur: 0.0,
            agx_look: AgxLook::Punchy,
        }
    }
}

/// Default value of one schema control.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ControlDefault {
    /// Numeric control default.
    Number(f64),
    /// Enumerated control default (serialized spelling).
    Choice(&'static str),
}

/// One tunable display setting: validation range plus labels for live-tuning interfaces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PostProcessingControl {
    /// Serialized camelCase key.
    pub key: &'static str,
    /// Short interface label.
    pub label: &'static str,
    /// Interface group heading.
    pub group: &'static str,
    /// One-sentence description.
    pub description: &'static str,
    /// Engine default.
    pub default: ControlDefault,
    /// Inclusive lower bound (exclusive when `exclusive_minimum`).
    pub minimum: Option<f64>,
    /// Whether `minimum` itself is rejected.
    pub exclusive_minimum: bool,
    /// Inclusive upper bound.
    pub maximum: Option<f64>,
    /// Convenient slider limit; authored values may exceed it up to `maximum`.
    pub slider_maximum: Option<f64>,
    /// Suggested slider increment.
    pub step: Option<f64>,
    /// Allowed spellings for enumerated controls; empty for numbers.
    pub choices: &'static [&'static str],
}

/// Shorthand constructor keeping the schema table readable.
const fn number(
    key: &'static str,
    label: &'static str,
    group: &'static str,
    default: f64,
    range: (Option<f64>, bool, Option<f64>, Option<f64>),
    step: f64,
    description: &'static str,
) -> PostProcessingControl {
    PostProcessingControl {
        key,
        label,
        group,
        description,
        default: ControlDefault::Number(default),
        minimum: range.0,
        exclusive_minimum: range.1,
        maximum: range.2,
        slider_maximum: range.3,
        step: Some(step),
        choices: &[],
    }
}

/// Shorthand constructor for enumerated controls.
const fn choice(
    key: &'static str,
    label: &'static str,
    group: &'static str,
    default: &'static str,
    choices: &'static [&'static str],
    description: &'static str,
) -> PostProcessingControl {
    PostProcessingControl {
        key,
        label,
        group,
        description,
        default: ControlDefault::Choice(default),
        minimum: None,
        exclusive_minimum: false,
        maximum: None,
        slider_maximum: None,
        step: None,
        choices,
    }
}

/// One source for validation, engine defaults, and live controls, in interface order.
// One control per row keeps the schema scannable as a table; rustfmt would split each row across many lines.
#[rustfmt::skip]
pub const POST_PROCESSING_SCHEMA: &[PostProcessingControl] = &[
    choice("toneMapper", "Tone Mapper", "Display", "agx", &["agx", "aces", "none"], "Display transform after linear HDR composition."),
    choice("agxLook", "AgX Look", "Display", "punchy", &["neutral", "punchy"], "Punchy adds the AgX power and saturation look before the display transform."),
    number("exposure", "Exposure", "Display", 1.0, (Some(0.0), false, None, Some(4.0)), 0.01, "Linear scene exposure before tone mapping."),
    number("antialiasStrength", "Antialias Strength", "Display", 1.0, (Some(0.0), false, Some(1.0), Some(1.0)), 0.01, "Edge-only scene smoothing; zero disables it."),
    number("bloomStrength", "Strength", "Bloom", 0.0, (Some(0.0), false, None, Some(3.0)), 0.01, "Bright-pass glow strength; zero disables its visual contribution."),
    number("bloomRadius", "Radius", "Bloom", 0.025, (Some(0.0), false, Some(0.25), Some(0.1)), 0.001, "Blur reach as a fraction of the shorter viewport axis."),
    number("bloomThreshold", "Threshold", "Bloom", 1.0, (Some(0.0), false, None, Some(4.0)), 0.01, "Linear luminance at which glow begins."),
    number("bloomAnamorphic", "Anamorphic", "Bloom", 0.0, (Some(0.0), false, Some(1.0), Some(1.0)), 0.01, "Stretches bloom horizontally into a lens-flare shape."),
    choice("bloomQuality", "Quality", "Bloom", "high", &["low", "medium", "high", "ultra"], "Two to five HDR pyramid levels; higher levels retain broader, smoother glow."),
    number("bloomScatter", "Scatter", "Bloom", 0.55, (Some(0.0), false, Some(1.0), Some(1.0)), 0.01, "Contribution of the wider bloom levels during reconstruction."),
    number("bloomLensFlare", "Lens Streaks", "Bloom", 0.0, (Some(0.0), false, None, Some(3.0)), 0.01, "Independent horizontal diffraction streaks generated from HDR highlights."),
    number("bloomKnee", "Soft Knee", "Bloom", 0.35, (Some(0.0), true, None, Some(2.0)), 0.01, "Width of the transition into bloom; must be positive."),
    number("saturation", "Saturation", "Grade", 1.0, (Some(0.0), false, None, Some(3.0)), 0.01, "Display saturation after tone mapping."),
    number("contrast", "Contrast", "Grade", 1.0, (Some(0.0), false, None, Some(3.0)), 0.01, "Display contrast around middle gray."),
    number("brightness", "Brightness", "Grade", 1.0, (Some(0.0), false, Some(2.0), Some(2.0)), 0.01, "Display brightness multiplier; zero is black and one preserves the tone-mapped image."),
    number("vibrance", "Vibrance", "Grade", 0.0, (Some(-1.0), false, Some(1.0), None), 0.01, "Adjusts muted colors more strongly than saturated colors."),
    number("blurRadius", "Gaussian Radius", "Blur", 0.0, (Some(0.0), false, Some(0.05), Some(0.05)), 0.001, "Full-image Gaussian blur as a fraction of the shorter viewport axis."),
    number("filmGrain", "Intensity", "Film Grain", 0.0, (Some(0.0), false, None, Some(0.2)), 0.001, "Luminance-aware grain intensity."),
    number("filmGrainSize", "Size", "Film Grain", 1.5, (Some(0.5), false, Some(8.0), Some(8.0)), 0.1, "Noise-cell size in physical pixels."),
    number("filmGrainSpeed", "Speed", "Film Grain", 18.0, (Some(0.0), false, None, Some(60.0)), 0.5, "Noise animation rate in cycles per second."),
    choice("filmGrainNoise", "Noise", "Film Grain", "decorrelated", &["analog", "decorrelated"], "Analog hash or averaged decorrelated hash grain; neither claims a blue-noise spectrum."),
    number("vignetteStrength", "Strength", "Vignette", 0.0, (Some(0.0), false, None, Some(1.0)), 0.01, "Edge darkening strength."),
    number("vignetteBlur", "Blur", "Vignette", 0.0, (Some(0.0), false, Some(1.0), Some(1.0)), 0.01, "Softens the image toward the vignette edge."),
    number("vignetteRadius", "Radius", "Vignette", 0.72, (Some(0.0), true, Some(1.0), Some(1.0)), 0.01, "Normalized radius where edge darkening begins."),
];

/// Named starting points shared by games; a profile stores only its own overrides on top of one of these.
pub fn preset_overrides(name: &str) -> Option<&'static [(&'static str, f64)]> {
    match name {
        "default" => Some(&[]),
        "soft" => Some(&[("bloomStrength", 0.25), ("contrast", 0.95), ("saturation", 0.95)]),
        "vivid" => Some(&[("bloomStrength", 0.6), ("saturation", 1.2), ("contrast", 1.05)]),
        _ => None,
    }
}

/// Names of every built-in preset, in interface order.
pub const PRESET_NAMES: &[&str] = &["default", "soft", "vivid"];

/// Validation failure for display settings or authored profiles. Messages match the TypeScript engine's intent.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PostProcessingError {
    /// Settings or profile were not a JSON object.
    #[error("Post-processing {0} must be an object.")]
    NotAnObject(&'static str),
    /// An override names a setting that does not exist.
    #[error("Unknown post-processing setting: {0}.")]
    UnknownSetting(String),
    /// An enumerated setting used an unsupported spelling.
    #[error("Post-processing {key} must be one of: {choices}.")]
    InvalidChoice {
        /// Setting key.
        key: &'static str,
        /// Comma-separated allowed spellings.
        choices: String,
    },
    /// A numeric setting was missing, non-numeric, or non-finite.
    #[error("Post-processing {0} must be finite.")]
    NotFinite(&'static str),
    /// A numeric setting fell below its minimum.
    #[error("Post-processing {key} {requirement}.")]
    BelowMinimum {
        /// Setting key.
        key: &'static str,
        /// Human-readable requirement.
        requirement: &'static str,
    },
    /// A numeric setting exceeded its maximum.
    #[error("Post-processing {key} exceeds {maximum}.")]
    AboveMaximum {
        /// Setting key.
        key: &'static str,
        /// Inclusive maximum.
        maximum: f64,
    },
    /// A profile contained a field other than `version`, `preset`, and `overrides`.
    #[error("Unknown post-processing profile field.")]
    UnknownProfileField,
    /// A profile used a version other than one.
    #[error("Unsupported post-processing profile version; expected 1.")]
    UnsupportedVersion,
    /// A profile named an unknown preset.
    #[error("Unknown post-processing preset.")]
    UnknownPreset,
}

impl PostProcessingConfig {
    /// Validate partial settings on top of engine defaults and return a complete record.
    ///
    /// Unknown keys and invalid values are rejected before any caller state changes. The input is never retained.
    pub fn normalize(overrides: &JsonRecord) -> Result<Self, PostProcessingError> {
        let mut merged = defaults_record();
        for (key, value) in overrides {
            if !POST_PROCESSING_SCHEMA.iter().any(|control| control.key == key) {
                return Err(PostProcessingError::UnknownSetting(key.clone()));
            }
            merged.insert(key.clone(), value.clone());
        }
        validate_record(&merged)?;

        // Every key is validated above, so deserialization cannot fail for schema reasons.
        serde_json::from_value(Value::Object(merged)).map_err(|_| PostProcessingError::NotAnObject("settings"))
    }

    /// Serialize these settings as a JSON record keyed by camelCase setting names.
    pub fn to_record(&self) -> JsonRecord {
        match serde_json::to_value(self) {
            Ok(Value::Object(record)) => record,
            _ => Map::new(),
        }
    }
}

/// Authored display profile: a preset name plus the game's own overrides.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PostProcessingProfile {
    /// Profile format version; always one.
    pub version: u64,
    /// Built-in preset the overrides apply on top of.
    pub preset: String,
    /// Validated overrides in canonical (sorted) key order.
    pub overrides: JsonRecord,
}

impl PostProcessingProfile {
    /// Validate serialized profile data and return a detached, canonically ordered copy.
    ///
    /// Unknown versions are rejected until a deliberate migration exists. Version-one `blue-noise` grain is migrated to
    /// `decorrelated` without modifying the input.
    pub fn validate(value: &Value) -> Result<Self, PostProcessingError> {
        let Value::Object(record) = value else {
            return Err(PostProcessingError::NotAnObject("profile"));
        };
        if record.keys().any(|key| !matches!(key.as_str(), "version" | "preset" | "overrides")) {
            return Err(PostProcessingError::UnknownProfileField);
        }
        if record.get("version").and_then(Value::as_f64) != Some(PROFILE_VERSION as f64) {
            return Err(PostProcessingError::UnsupportedVersion);
        }
        let preset = match record.get("preset") {
            Some(Value::String(name)) if preset_overrides(name).is_some() => name.clone(),
            _ => return Err(PostProcessingError::UnknownPreset),
        };
        let Some(Value::Object(authored)) = record.get("overrides") else {
            return Err(PostProcessingError::NotAnObject("overrides"));
        };

        let mut overrides = authored.clone();
        if overrides.get("filmGrainNoise").and_then(Value::as_str) == Some(LEGACY_BLUE_NOISE) {
            overrides.insert("filmGrainNoise".to_owned(), Value::String("decorrelated".to_owned()));
        }
        PostProcessingConfig::normalize(&overrides)?;

        let mut sorted: Vec<(String, Value)> = overrides.into_iter().collect();
        sorted.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(Self { version: PROFILE_VERSION, preset, overrides: sorted.into_iter().collect() })
    }

    /// Resolve engine defaults, then the named preset, then this profile's overrides into complete settings.
    pub fn resolve(&self) -> Result<PostProcessingConfig, PostProcessingError> {
        let preset = preset_overrides(&self.preset).ok_or(PostProcessingError::UnknownPreset)?;
        let mut combined = JsonRecord::new();
        for (key, value) in preset {
            combined.insert((*key).to_owned(), Value::from(*value));
        }
        for (key, value) in &self.overrides {
            combined.insert(key.clone(), value.clone());
        }
        PostProcessingConfig::normalize(&combined)
    }
}

/// Engine defaults as a JSON record, used as the base for every normalization.
fn defaults_record() -> JsonRecord {
    PostProcessingConfig::default().to_record()
}

/// Check every schema control in a complete record against its type and range.
fn validate_record(record: &JsonRecord) -> Result<(), PostProcessingError> {
    for control in POST_PROCESSING_SCHEMA {
        let value = record.get(control.key);

        // Enumerated controls accept only their listed spellings.
        if !control.choices.is_empty() {
            let accepted = value.and_then(Value::as_str).is_some_and(|text| control.choices.contains(&text));
            if !accepted {
                return Err(PostProcessingError::InvalidChoice {
                    key: control.key,
                    choices: control.choices.join(", "),
                });
            }
            continue;
        }

        let Some(number) = value.and_then(Value::as_f64).filter(|number| number.is_finite()) else {
            return Err(PostProcessingError::NotFinite(control.key));
        };
        if let Some(minimum) = control.minimum {
            let below = if control.exclusive_minimum { number <= minimum } else { number < minimum };
            if below {
                let requirement =
                    if control.exclusive_minimum { "must be greater than zero" } else { "cannot be negative" };
                return Err(PostProcessingError::BelowMinimum { key: control.key, requirement });
            }
        }
        if let Some(maximum) = control.maximum
            && number > maximum
        {
            return Err(PostProcessingError::AboveMaximum { key: control.key, maximum });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_match_schema() {
        let defaults = defaults_record();
        for control in POST_PROCESSING_SCHEMA {
            let value = defaults.get(control.key).expect("every schema key has a default");
            match control.default {
                ControlDefault::Number(expected) => {
                    assert!((value.as_f64().unwrap() - expected).abs() < 1e-6, "{}", control.key)
                }
                ControlDefault::Choice(expected) => assert_eq!(value.as_str(), Some(expected), "{}", control.key),
            }
        }
        assert_eq!(defaults.len(), POST_PROCESSING_SCHEMA.len());
    }

    #[test]
    fn profile_resolution_applies_preset_then_overrides() {
        let profile = PostProcessingProfile::validate(&json!({
            "version": 1,
            "preset": "vivid",
            "overrides": {"saturation": 0.9, "filmGrainNoise": "blue-noise"}
        }))
        .unwrap();
        assert_eq!(profile.overrides.keys().collect::<Vec<_>>(), ["filmGrainNoise", "saturation"]);
        let config = profile.resolve().unwrap();
        assert_eq!(config.bloom_strength, 0.6);
        assert_eq!(config.saturation, 0.9);
        assert_eq!(config.film_grain_noise, FilmGrainNoise::Decorrelated);
    }

    #[test]
    fn invalid_values_are_rejected() {
        let reject = |overrides: Value| PostProcessingConfig::normalize(overrides.as_object().unwrap()).unwrap_err();
        assert_eq!(
            reject(json!({"bloomKnee": 0})),
            PostProcessingError::BelowMinimum { key: "bloomKnee", requirement: "must be greater than zero" }
        );
        assert_eq!(
            reject(json!({"exposure": -1})),
            PostProcessingError::BelowMinimum { key: "exposure", requirement: "cannot be negative" }
        );
        assert_eq!(
            reject(json!({"brightness": 3})),
            PostProcessingError::AboveMaximum { key: "brightness", maximum: 2.0 }
        );
        assert_eq!(reject(json!({"glow": 1})), PostProcessingError::UnknownSetting("glow".into()));
        assert!(matches!(reject(json!({"toneMapper": "filmic"})), PostProcessingError::InvalidChoice { .. }));
        assert_eq!(
            PostProcessingProfile::validate(&json!({"version": 2, "preset": "default", "overrides": {}})).unwrap_err(),
            PostProcessingError::UnsupportedVersion
        );
    }
}
