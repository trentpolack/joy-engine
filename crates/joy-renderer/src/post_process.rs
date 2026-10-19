// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Post-processing settings: the values the renderer's display chain applies, the control table that validates and
//! describes them, sparse overrides, and weighted blending between them.
//!
//! [`PostProcessSettings`] is a complete set of values; the renderer holds one and applies it every frame
//! ([`Renderer::set_post_process`](crate::Renderer::set_post_process)). [`PostProcessOverrides`] is a validated, sparse
//! set of values, such as the ones a level's `postProcess` volume component authors; omitted settings inherit.
//! [`PostProcessSettings::blend_toward`] moves settings toward overrides by a weight, so volumes can fade in and out.
//! Serialized names and ranges are the `postProcess` component's authored contract.

use serde::{
    Deserialize,
    Serialize,
};
use serde_json::{
    Map,
    Value,
};

use joy_core::JsonRecord;
use joy_core::math::deterministic;

/// Key of the exposure setting, which blends in log space because exposure is perceived multiplicatively.
const EXPOSURE_KEY: &str = "exposure";

/// Weight at and above which a blend adopts an override's discrete choice (tone mapper, grain noise, and so on).
const CHOICE_SWITCH_WEIGHT: f32 = 0.5;

// ---------------------------------------------------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------------------------------------------------

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

/// Complete, validated post-processing values. Lengths are fractions of the shorter viewport axis unless noted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostProcessSettings {
    /// Display transform after linear HDR composition.
    pub tone_mapper: ToneMapper,
    /// Linear scene exposure before tone mapping.
    pub exposure: f32,
    /// AgX grading look.
    pub agx_look: AgxLook,

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


    /// Edge-only scene smoothing in `[0, 1]`; zero disables it.
    pub antialias_strength: f32,

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
}

impl PostProcessSettings {
    /// The engine's defaults: AgX with the punchy look, edge antialiasing, and every optional effect off.
    pub const DEFAULT: Self = Self {
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
    };

    /// Linear scene color resolved straight to the display: no tone curve, antialiasing, or optional effects.
    pub const NEUTRAL: Self = Self { tone_mapper: ToneMapper::None, antialias_strength: 0.0, ..Self::DEFAULT };

    /// These settings with `overrides` applied in full.
    pub fn with_overrides(&self, overrides: &PostProcessOverrides) -> Self {
        self.blend_toward(overrides, 1.0)
    }

    /// These settings moved toward `overrides` by `weight` in `[0, 1]`; settings the overrides omit are unchanged.
    ///
    /// Numbers interpolate linearly, except exposure, which interpolates in log space. Discrete choices adopt the
    /// override at a weight of one half or more.
    pub fn blend_toward(&self, overrides: &PostProcessOverrides, weight: f32) -> Self {
        let weight = weight.clamp(0.0, 1.0);
        if weight == 0.0 || overrides.is_empty() {
            return self.clone();
        }

        let mut record = self.to_record();
        for (key, target) in overrides.record() {
            let blended = match (record.get(key).and_then(Value::as_f64), target.as_f64()) {
                (Some(from), Some(to)) => Value::from(blend_number(key, from, to, weight)),
                _ if weight >= CHOICE_SWITCH_WEIGHT => target.clone(),
                _ => continue,
            };
            record.insert(key.clone(), blended);
        }

        // Overrides are validated against the control table and blends stay between valid values, so this always
        // deserializes; keep the current settings rather than panic if that invariant ever breaks.
        serde_json::from_value(Value::Object(record)).unwrap_or_else(|_| self.clone())
    }

    /// These settings as a JSON record keyed by camelCase setting names.
    pub fn to_record(&self) -> JsonRecord {
        match serde_json::to_value(self) {
            Ok(Value::Object(record)) => record,
            _ => Map::new(),
        }
    }
}

impl Default for PostProcessSettings {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// One setting interpolated from `from` to `to` by `weight`.
fn blend_number(key: &str, from: f64, to: f64, weight: f32) -> f64 {
    let weight = f64::from(weight);
    if weight >= 1.0 {
        return to;
    }
    if key == EXPOSURE_KEY && from > 0.0 && to > 0.0 {
        let log_from = deterministic::log2(from);
        let log_to = deterministic::log2(to);
        return deterministic::exp2(log_from + ((log_to - log_from) * weight));
    }
    from + ((to - from) * weight)
}

// ---------------------------------------------------------------------------------------------------------------------
// Overrides
// ---------------------------------------------------------------------------------------------------------------------

/// A validated, sparse set of post-processing values keyed by camelCase setting names; omitted settings inherit.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PostProcessOverrides {
    values: JsonRecord,
}

impl PostProcessOverrides {
    /// No overrides.
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate every key and value of `record` against [`POST_PROCESS_CONTROLS`]; the record is copied.
    pub fn from_record(record: &JsonRecord) -> Result<Self, PostProcessError> {
        let mut overrides = Self::new();
        for (key, value) in record {
            overrides.set(key, value.clone())?;
        }
        Ok(overrides)
    }

    /// Set one value after validating it; an invalid key or value leaves the overrides unchanged.
    pub fn set(&mut self, key: &str, value: Value) -> Result<(), PostProcessError> {
        let control = post_process_control(key).ok_or_else(|| PostProcessError::UnknownSetting(key.to_owned()))?;
        validate_value(control, &value)?;
        self.values.insert(control.key.to_owned(), value);
        Ok(())
    }

    /// Remove one value so it inherits again; returns whether it was present.
    pub fn remove(&mut self, key: &str) -> bool {
        self.values.shift_remove(key).is_some()
    }

    /// One overridden value.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    /// Whether `key` is overridden.
    pub fn contains(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    /// Whether nothing is overridden.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// The overridden values in authored order.
    pub fn record(&self) -> &JsonRecord {
        &self.values
    }

    /// Whether both hold the same settings with equal values, comparing numbers by value (`2` equals `2.0`).
    pub fn same_values(&self, other: &Self) -> bool {
        self.values.len() == other.values.len()
            && self.values.iter().all(|(key, value)| {
                other.values.get(key).is_some_and(|other| match (value.as_f64(), other.as_f64()) {
                    (Some(left), Some(right)) => left == right,
                    _ => value == other,
                })
            })
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Controls
// ---------------------------------------------------------------------------------------------------------------------

/// Default value of one control.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PostProcessControlDefault {
    /// Numeric control default.
    Number(f64),
    /// Enumerated control default (serialized spelling).
    Choice(&'static str),
}

/// One post-processing setting: its validation range plus labels for authoring and tuning interfaces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PostProcessControl {
    /// Serialized camelCase key.
    pub key: &'static str,
    /// Short interface label.
    pub label: &'static str,
    /// Interface group heading.
    pub group: &'static str,
    /// One-sentence description.
    pub description: &'static str,
    /// Engine default.
    pub default: PostProcessControlDefault,
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

/// Shorthand constructor keeping the control table readable.
const fn number(
    key: &'static str,
    label: &'static str,
    group: &'static str,
    default: f64,
    range: (Option<f64>, bool, Option<f64>, Option<f64>),
    step: f64,
    description: &'static str,
) -> PostProcessControl {
    PostProcessControl {
        key,
        label,
        group,
        description,
        default: PostProcessControlDefault::Number(default),
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
) -> PostProcessControl {
    PostProcessControl {
        key,
        label,
        group,
        description,
        default: PostProcessControlDefault::Choice(default),
        minimum: None,
        exclusive_minimum: false,
        maximum: None,
        slider_maximum: None,
        step: None,
        choices,
    }
}

/// One source for validation, defaults, and authoring controls, in interface order.
// One control per row keeps the table scannable; rustfmt would split each row across many lines.
#[rustfmt::skip]
pub const POST_PROCESS_CONTROLS: &[PostProcessControl] = &[
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

/// The control for a setting key, or `None` for an unknown key.
pub fn post_process_control(key: &str) -> Option<&'static PostProcessControl> {
    POST_PROCESS_CONTROLS.iter().find(|control| control.key == key)
}

// ---------------------------------------------------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------------------------------------------------

/// A post-processing value that does not fit its control.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PostProcessError {
    /// A key names a setting that does not exist.
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
    /// A numeric setting was non-numeric or non-finite.
    #[error("Post-processing {0} must be finite.")]
    NotFinite(&'static str),
    /// A numeric setting fell below its minimum.
    #[error("Post-processing {key} must be {} {minimum}.", if *exclusive { "greater than" } else { "at least" })]
    BelowMinimum {
        /// Setting key.
        key: &'static str,
        /// The minimum.
        minimum: f64,
        /// Whether the minimum itself is excluded.
        exclusive: bool,
    },
    /// A numeric setting exceeded its maximum.
    #[error("Post-processing {key} exceeds {maximum}.")]
    AboveMaximum {
        /// Setting key.
        key: &'static str,
        /// Inclusive maximum.
        maximum: f64,
    },
}

/// Check one value against its control's type and range.
fn validate_value(control: &PostProcessControl, value: &Value) -> Result<(), PostProcessError> {
    // Enumerated controls accept only their listed spellings.
    if !control.choices.is_empty() {
        let accepted = value.as_str().is_some_and(|text| control.choices.contains(&text));
        if !accepted {
            return Err(PostProcessError::InvalidChoice { key: control.key, choices: control.choices.join(", ") });
        }
        return Ok(());
    }

    let Some(number) = value.as_f64().filter(|number| number.is_finite()) else {
        return Err(PostProcessError::NotFinite(control.key));
    };
    if let Some(minimum) = control.minimum {
        let exclusive = control.exclusive_minimum;
        let below = if exclusive { number <= minimum } else { number < minimum };
        if below {
            return Err(PostProcessError::BelowMinimum { key: control.key, minimum, exclusive });
        }
    }
    if let Some(maximum) = control.maximum
        && number > maximum
    {
        return Err(PostProcessError::AboveMaximum { key: control.key, maximum });
    }
    Ok(())
}

#[cfg(test)]
#[path = "post_process.test.rs"]
mod tests;
