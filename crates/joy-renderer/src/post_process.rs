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

/// Weight at and above which a blend adopts an override's discrete choice (tone mapper, bloom quality, and so on).
const CHOICE_SWITCH_WEIGHT: f32 = 0.5;

// ---------------------------------------------------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------------------------------------------------

/// Display transform applied after exposure and bloom.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToneMapper {
    /// A parametric toe, linear section, and shoulder per channel (default); see the `filmic*` settings.
    Filmic,
    /// AgX, with an optional punchy look.
    Agx,
    /// Exposed linear light with a display clamp.
    None,
}

/// AgX grading look.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgxLook {
    /// The base AgX curve.
    Default,
    /// Adds AgX's power and saturation look before the display transform.
    Punchy,
}

/// Bloom filtering quality; glow reach comes from [`PostProcessSettings::bloom_radius`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BloomQuality {
    /// Four texels per downsample and upsample.
    Low,
    /// Thirteen-tap downsamples and nine-tap tent upsamples.
    Medium,
    /// Medium filters plus firefly suppression, so small, very bright highlights do not flicker.
    High,
    /// High, with the first pyramid level at full resolution for a tighter glow core.
    Ultra,
}

/// Complete, validated post-processing values. Lengths are fractions of the shorter viewport axis unless noted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostProcessSettings {
    /// Display transform after exposure and bloom.
    pub tone_mapper: ToneMapper,
    /// Linear multiplier on scene light before bloom and the display transform; adaptive exposure multiplies it.
    pub exposure: f32,
    /// AgX grading look.
    pub agx_look: AgxLook,

    /// Filmic: slope of the linear section.
    pub filmic_contrast: f32,
    /// Filmic: exposed linear light where the toe ends and the linear section begins.
    pub filmic_linear_start: f32,
    /// Filmic: fraction of the range above the linear start that stays linear before the shoulder.
    pub filmic_linear_length: f32,
    /// Filmic: toe exponent; higher values deepen shadows.
    pub filmic_toe: f32,

    /// Glow strength; zero disables bloom.
    pub bloom_strength: f32,
    /// Glow reach as a fraction of the shorter viewport axis.
    pub bloom_radius: f32,
    /// Exposed linear luminance at which glow begins.
    pub bloom_threshold: f32,
    /// Width of the soft transition into bloom; always positive.
    pub bloom_knee: f32,
    /// Bloom filtering quality.
    pub bloom_quality: BloomQuality,
    /// How much of each wider pyramid level blends into the next finer one, in `[0, 1]`.
    pub bloom_scatter: f32,

    /// Corner darkening strength in `[0, 1]`.
    pub vignette_strength: f32,
    /// Fraction of the center-to-corner distance where darkening begins.
    pub vignette_radius: f32,
}

impl PostProcessSettings {
    /// The engine's defaults: the filmic curve at unit exposure, with bloom and vignette off.
    pub const DEFAULT: Self = Self {
        tone_mapper: ToneMapper::Filmic,
        exposure: 1.0,
        agx_look: AgxLook::Default,
        filmic_contrast: 1.0,
        filmic_linear_start: 0.22,
        filmic_linear_length: 0.4,
        filmic_toe: 1.33,
        bloom_strength: 0.0,
        bloom_radius: 0.05,
        bloom_threshold: 1.0,
        bloom_knee: 0.35,
        bloom_quality: BloomQuality::High,
        bloom_scatter: 0.7,
        vignette_strength: 0.0,
        vignette_radius: 0.72,
    };

    /// Linear scene color resolved straight to the display: no tone curve or optional effects.
    pub const NEUTRAL: Self = Self {
        tone_mapper: ToneMapper::None,
        ..Self::DEFAULT
    };

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
    choice("toneMapper", "Tone Mapper", "Display", "filmic", &["filmic", "agx", "none"], "Display transform: the parametric filmic curve, AgX, or none."),
    choice("agxLook", "AgX Look", "Display", "default", &["default", "punchy"], "Punchy adds AgX's power and saturation look before the display transform."),
    number("exposure", "Exposure", "Display", 1.0, (Some(0.0), false, None, Some(4.0)), 0.01, "Linear multiplier on scene light before bloom and the display transform; adaptive exposure multiplies it."),
    number("filmicContrast", "Contrast", "Filmic", 1.0, (Some(0.1), false, Some(3.0), Some(2.0)), 0.01, "Slope of the curve's linear section."),
    number("filmicLinearStart", "Linear Start", "Filmic", 0.22, (Some(0.01), false, Some(0.9), Some(0.6)), 0.01, "Exposed linear light where the toe ends and the linear section begins."),
    number("filmicLinearLength", "Linear Length", "Filmic", 0.4, (Some(0.0), false, Some(0.95), Some(0.95)), 0.01, "Fraction of the range above the linear start that stays linear before the shoulder."),
    number("filmicToe", "Toe", "Filmic", 1.33, (Some(1.0), false, Some(3.0), Some(3.0)), 0.01, "Toe exponent; higher values deepen shadows."),
    number("bloomStrength", "Strength", "Bloom", 0.0, (Some(0.0), false, None, Some(3.0)), 0.01, "Glow strength; zero disables bloom."),
    number("bloomRadius", "Radius", "Bloom", 0.05, (Some(0.0), true, Some(0.5), Some(0.25)), 0.001, "Glow reach as a fraction of the shorter viewport axis."),
    number("bloomThreshold", "Threshold", "Bloom", 1.0, (Some(0.0), false, None, Some(4.0)), 0.01, "Exposed linear luminance at which glow begins."),
    number("bloomKnee", "Soft Knee", "Bloom", 0.35, (Some(0.0), true, None, Some(2.0)), 0.01, "Width of the transition into bloom; must be positive."),
    choice("bloomQuality", "Quality", "Bloom", "high", &["low", "medium", "high", "ultra"], "Filtering: low reads four texels per step, medium widens the filters, high suppresses fireflies, and ultra starts at full resolution."),
    number("bloomScatter", "Scatter", "Bloom", 0.7, (Some(0.0), false, Some(1.0), Some(1.0)), 0.01, "How much of each wider pyramid level blends into the next finer one; higher values spread the glow."),
    number("vignetteStrength", "Strength", "Vignette", 0.0, (Some(0.0), false, Some(1.0), Some(1.0)), 0.01, "Corner darkening strength."),
    number("vignetteRadius", "Radius", "Vignette", 0.72, (Some(0.0), false, Some(1.0), Some(1.0)), 0.01, "Fraction of the center-to-corner distance where darkening begins."),
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
