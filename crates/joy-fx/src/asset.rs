// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! `.joyfx` particle effect documents: validation, normalization, and compilation into shared definitions.
//!
//! [`compile_particle_effect`] validates a JSON document exactly like the TypeScript `compileParticleEffect` (same
//! limits, same check order, and the same messages), normalizes it (defaults filled, bursts sorted by time), and compiles
//! every emitter's spawn and update scripts. The result is an immutable, cheaply cloned [`CompiledParticleEffect`] that
//! any number of [`ParticleEffect`](crate::ParticleEffect) instances share.
//!
//! Units: effects use caller-defined world units, seconds, radians, and linear HDR RGB.

use std::sync::Arc;

use indexmap::IndexMap;
use joy_core::format_number;
use serde_json::{
    Map,
    Value,
};

use crate::constants::{
    ParticleParameterType,
    ParticleRenderer,
    ParticleValueKind,
};
use crate::script::{
    FIXED_SLOT_COUNT,
    PARTICLE_FIELDS,
    PARTICLE_INPUTS,
    ParticleScript,
    ParticleScriptError,
    compile_particle_script,
};
use crate::texture_source::{
    ParticleTextureDimensions,
    ParticleTextureError,
    read_particle_texture_source,
};

/// The only supported `.joyfx` format version.
pub const PARTICLE_EFFECT_VERSION: u32 = 1;

/// Largest particle capacity an effect may declare.
pub const MAXIMUM_EFFECT_PARTICLES: u32 = 4096;

/// Largest number of parameters per effect.
const MAXIMUM_PARAMETERS: usize = 32;

/// Largest number of emitters per effect.
const MAXIMUM_EMITTERS: usize = 32;

/// Largest number of bursts per emitter.
const MAXIMUM_BURSTS: usize = 128;

/// Largest number of distinct embedded PNG sources per effect.
const MAXIMUM_TEXTURE_SOURCES: usize = 8;

/// Largest total embedded texture text per effect, in characters.
const MAXIMUM_TEXTURE_TEXT: usize = 4 * 1024 * 1024;

/// Largest magnitude of a parameter bound or value.
const MAXIMUM_PARAMETER_MAGNITUDE: f64 = 1e6;

/// Serialized name of an emitter light's shadow flag.
pub const PARTICLE_LIGHT_CASTS_SHADOW: &str = "castsShadow";

/// Upper bounds of color parameter channels: linear RGB may exceed one for HDR tints, and alpha is opacity.
const COLOR_PARAMETER_LIMITS: [f64; 4] =
    [MAXIMUM_PARAMETER_MAGNITUDE, MAXIMUM_PARAMETER_MAGNITUDE, MAXIMUM_PARAMETER_MAGNITUDE, 1.0];

/// Names a parameter may not take: script fields, inputs, keywords, and JavaScript prototype keys.
const RESERVED_NAMES: [&str; 6] = ["let", "if", "else", "constructor", "prototype", "__proto__"];

/// An authored effect parameter, normalized. Scalars serialize without a `type` field.
#[derive(Clone, Debug, PartialEq)]
pub enum ParticleParameter {
    /// One number.
    Scalar {
        /// Default value in `[min, max]`.
        value: f64,
        /// Lower bound.
        min: f64,
        /// Upper bound.
        max: f64,
    },
    /// XYZ components, readable in scripts as `nameX`/`nameY`/`nameZ` or the vector `name`.
    Vector3 {
        /// Default components within the bounds.
        value: [f64; 3],
        /// Per-axis lower bounds.
        min: [f64; 3],
        /// Per-axis upper bounds.
        max: [f64; 3],
    },
    /// Linear RGBA components, readable in scripts as the color `name`: RGB is nonnegative and may exceed one for HDR
    /// tints, and alpha is in `[0, 1]`.
    Color {
        /// Default components within the bounds.
        value: [f64; 4],
        /// Per-channel lower bounds.
        min: [f64; 4],
        /// Per-channel upper bounds.
        max: [f64; 4],
    },
}

/// A parameter value supplied when creating an effect instance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParticleParameterValue {
    /// For scalar parameters.
    Scalar(f64),
    /// For vector3 parameters.
    Vector3([f64; 3]),
    /// For color parameters.
    Color([f64; 4]),
}

impl ParticleParameterValue {
    /// Interpret a JSON number (scalar), three-number array (vector3), or four-number array (color); `None` otherwise.
    pub fn from_json(value: &Value) -> Option<Self> {
        match value {
            Value::Number(number) => number.as_f64().map(Self::Scalar),
            Value::Array(items) => {
                let numbers: Option<Vec<f64>> = items.iter().map(Value::as_f64).collect();
                match numbers?.as_slice() {
                    [x, y, z] => Some(Self::Vector3([*x, *y, *z])),
                    [r, g, b, a] => Some(Self::Color([*r, *g, *b, *a])),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Components in script storage order.
    pub fn components(&self) -> &[f64] {
        match self {
            Self::Scalar(value) => std::slice::from_ref(value),
            Self::Vector3(values) => values,
            Self::Color(values) => values,
        }
    }
}

impl ParticleParameter {
    /// The serialized parameter type.
    pub fn parameter_type(&self) -> ParticleParameterType {
        match self {
            Self::Scalar { .. } => ParticleParameterType::Scalar,
            Self::Vector3 { .. } => ParticleParameterType::Vector3,
            Self::Color { .. } => ParticleParameterType::Color,
        }
    }

    /// The authored default.
    pub fn default_value(&self) -> ParticleParameterValue {
        match self {
            Self::Scalar { value, .. } => ParticleParameterValue::Scalar(*value),
            Self::Vector3 { value, .. } => ParticleParameterValue::Vector3(*value),
            Self::Color { value, .. } => ParticleParameterValue::Color(*value),
        }
    }

    /// Whether `value` has this parameter's shape, with finite components inside its bounds.
    pub fn accepts(&self, value: &ParticleParameterValue) -> bool {
        let within = |values: &[f64], min: &[f64], max: &[f64]| {
            values
                .iter()
                .zip(min.iter().zip(max))
                .all(|(value, (min, max))| value.is_finite() && value >= min && value <= max)
        };
        match (self, value) {
            (Self::Scalar { min, max, .. }, ParticleParameterValue::Scalar(value)) => {
                within(&[*value], &[*min], &[*max])
            }
            (Self::Vector3 { min, max, .. }, ParticleParameterValue::Vector3(values)) => within(values, min, max),
            (Self::Color { min, max, .. }, ParticleParameterValue::Color(values)) => within(values, min, max),
            _ => false,
        }
    }

    /// Script input names of this parameter's components (`speed`; `windX`…; `tint.r`…).
    pub fn input_names(&self, name: &str) -> Vec<String> {
        match self {
            Self::Scalar { .. } => vec![name.to_owned()],
            Self::Vector3 { .. } => ["X", "Y", "Z"].iter().map(|axis| format!("{name}{axis}")).collect(),
            Self::Color { .. } => ["r", "g", "b", "a"].iter().map(|channel| format!("{name}.{channel}")).collect(),
        }
    }

    fn to_json(&self) -> Value {
        let mut record = Map::new();
        let (value, min, max): (Vec<f64>, Vec<f64>, Vec<f64>) = match self {
            Self::Scalar { value, min, max } => {
                record.insert("min".to_owned(), json_number(*min));
                record.insert("max".to_owned(), json_number(*max));
                record.insert("value".to_owned(), json_number(*value));
                return Value::Object(record);
            }
            Self::Vector3 { value, min, max } => (value.to_vec(), min.to_vec(), max.to_vec()),
            Self::Color { value, min, max } => (value.to_vec(), min.to_vec(), max.to_vec()),
        };
        record.insert("type".to_owned(), Value::from(self.parameter_type().as_str()));
        record.insert("min".to_owned(), json_numbers(&min));
        record.insert("max".to_owned(), json_numbers(&max));
        record.insert("value".to_owned(), json_numbers(&value));
        Value::Object(record)
    }
}

/// A timed burst: `count` particles once the emitter clock reaches `time` seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleBurst {
    /// Seconds after the effect starts, within the emitter duration.
    pub time: f64,
    /// Particles emitted, 1–4096.
    pub count: u32,
}

/// A death child: `count` particles of `emitter` spawned where a particle expires.
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleChild {
    /// Child emitter id.
    pub emitter: String,
    /// Particles per event, 1–4096.
    pub count: u32,
}

/// A trail child: `count` particles of `emitter` spawned every `interval` seconds of a particle's life.
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleTrail {
    /// Child emitter id.
    pub emitter: String,
    /// Particles per event, 1–4096.
    pub count: u32,
    /// Seconds between events, at least one fixed step.
    pub interval: f64,
}

/// An embedded PNG texture or flipbook atlas.
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleTexture {
    /// `data:image/png;base64,…` source; shared, since sources can be large.
    pub source: Arc<str>,
    /// Atlas columns, 1–64 and at most the image width.
    pub columns: u32,
    /// Atlas rows, 1–64 and at most the image height.
    pub rows: u32,
    /// Frames used, row-major from the top-left, at most `columns * rows`.
    pub frames: u32,
    /// Flipbook frames per second; zero spreads the frames over each particle's lifetime.
    pub fps: f64,
    /// Whether flipbooks wrap (serialized as `loop`); otherwise they hold the last frame.
    pub looping: bool,
}

/// Point-light emission for an emitter's particles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleLight {
    /// Initial `lightIntensity`, 0–1000.
    pub intensity: f64,
    /// Initial `lightRange` in world units, 0.01–10000.
    pub range: f64,
    /// Whether the emitted lights cast soft shadows (serialized as `castsShadow`, written only when set). Off by
    /// default: particle lights are numerous and short-lived, and six shadow faces per light are costly, but an effect
    /// such as a lantern or a muzzle flash can opt in like any other point light.
    pub casts_shadow: bool,
}

/// One normalized emitter.
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleEmitterAsset {
    /// Unique identifier within the effect.
    pub id: String,
    /// Disabled emitters never spawn.
    pub enabled: bool,
    /// Root emitters spawn on the effect clock; others spawn only as trail or death children.
    pub root: bool,
    /// Seconds of continuous emission, 0–30.
    pub duration: f64,
    /// Particles per second during `duration`, 0–2000.
    pub rate: f64,
    /// Bursts sorted by time (stable for equal times).
    pub bursts: Vec<ParticleBurst>,
    /// Default particle lifetime in seconds.
    pub lifetime: f64,
    /// Geometry family.
    pub renderer: ParticleRenderer,
    /// Embedded texture; required by textured and flipbook renderers.
    pub texture: Option<ParticleTexture>,
    /// Particle point lights.
    pub light: Option<ParticleLight>,
    /// Script run once per new particle.
    pub spawn: String,
    /// Script run every fixed step before integration.
    pub update: String,
    /// Children spawned along a particle's life.
    pub trail: Option<ParticleTrail>,
    /// Children spawned when a particle expires.
    pub death: Option<ParticleChild>,
}

impl ParticleEmitterAsset {
    fn to_json(&self) -> Value {
        let mut record = Map::new();
        record.insert("id".to_owned(), Value::from(self.id.clone()));
        record.insert("enabled".to_owned(), Value::Bool(self.enabled));
        record.insert("root".to_owned(), Value::Bool(self.root));
        record.insert("duration".to_owned(), json_number(self.duration));
        record.insert("rate".to_owned(), json_number(self.rate));
        let bursts = self
            .bursts
            .iter()
            .map(|burst| {
                let mut entry = Map::new();
                entry.insert("time".to_owned(), json_number(burst.time));
                entry.insert("count".to_owned(), Value::from(burst.count));
                Value::Object(entry)
            })
            .collect();
        record.insert("bursts".to_owned(), Value::Array(bursts));
        record.insert("lifetime".to_owned(), json_number(self.lifetime));
        record.insert("renderer".to_owned(), Value::from(self.renderer.as_str()));
        record.insert("spawn".to_owned(), Value::from(self.spawn.clone()));
        record.insert("update".to_owned(), Value::from(self.update.clone()));
        if let Some(light) = &self.light {
            let mut entry = Map::new();
            entry.insert("intensity".to_owned(), json_number(light.intensity));
            entry.insert("range".to_owned(), json_number(light.range));
            if light.casts_shadow {
                entry.insert(PARTICLE_LIGHT_CASTS_SHADOW.to_owned(), Value::Bool(true));
            }
            record.insert("light".to_owned(), Value::Object(entry));
        }
        if let Some(texture) = &self.texture {
            let mut entry = Map::new();
            entry.insert("source".to_owned(), Value::from(texture.source.as_ref()));
            entry.insert("columns".to_owned(), Value::from(texture.columns));
            entry.insert("rows".to_owned(), Value::from(texture.rows));
            entry.insert("frames".to_owned(), Value::from(texture.frames));
            entry.insert("fps".to_owned(), json_number(texture.fps));
            entry.insert("loop".to_owned(), Value::Bool(texture.looping));
            record.insert("texture".to_owned(), Value::Object(entry));
        }
        if let Some(trail) = &self.trail {
            let mut entry = Map::new();
            entry.insert("emitter".to_owned(), Value::from(trail.emitter.clone()));
            entry.insert("count".to_owned(), Value::from(trail.count));
            entry.insert("interval".to_owned(), json_number(trail.interval));
            record.insert("trail".to_owned(), Value::Object(entry));
        }
        if let Some(death) = &self.death {
            let mut entry = Map::new();
            entry.insert("emitter".to_owned(), Value::from(death.emitter.clone()));
            entry.insert("count".to_owned(), Value::from(death.count));
            record.insert("death".to_owned(), Value::Object(entry));
        }
        Value::Object(record)
    }
}

/// A normalized `.joyfx` document.
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleEffectAsset {
    /// Always [`PARTICLE_EFFECT_VERSION`].
    pub version: u32,
    /// Display name, 1–80 characters.
    pub name: String,
    /// Live particle capacity, 1–4096; spawns beyond it are dropped and counted.
    pub max_particles: u32,
    /// Parameters in authored order.
    pub parameters: IndexMap<String, ParticleParameter>,
    /// Emitters in authored order (1–32).
    pub emitters: Vec<ParticleEmitterAsset>,
}

impl ParticleEffectAsset {
    /// Serialize in the TypeScript key order, with integral numbers written as integers like `JSON.stringify`.
    pub fn to_json(&self) -> Value {
        let mut record = Map::new();
        record.insert("version".to_owned(), Value::from(self.version));
        record.insert("name".to_owned(), Value::from(self.name.clone()));
        record.insert("maxParticles".to_owned(), Value::from(self.max_particles));
        let parameters = self.parameters.iter().map(|(name, parameter)| (name.clone(), parameter.to_json())).collect();
        record.insert("parameters".to_owned(), Value::Object(parameters));
        record.insert(
            "emitters".to_owned(),
            Value::Array(self.emitters.iter().map(ParticleEmitterAsset::to_json).collect()),
        );
        Value::Object(record)
    }
}

/// A compiled emitter: its normalized document, texture dimensions, and programs.
#[derive(Clone, Debug)]
pub struct CompiledEmitter {
    /// The normalized emitter document.
    pub asset: ParticleEmitterAsset,
    /// Pixel dimensions of the embedded texture, when present.
    pub texture_dimensions: Option<ParticleTextureDimensions>,
    /// Compiled spawn script.
    pub spawn_program: ParticleScript,
    /// Compiled update script.
    pub update_program: ParticleScript,
    // Resolved child emitter indices.
    pub(crate) trail_index: Option<usize>,
    pub(crate) death_index: Option<usize>,
}

/// An immutable, validated effect definition shared by effect instances; cloning shares the same definition.
#[derive(Clone, Debug)]
pub struct CompiledParticleEffect {
    inner: Arc<CompiledEffectInner>,
}

#[derive(Debug)]
struct CompiledEffectInner {
    asset: ParticleEffectAsset,
    emitters: Vec<CompiledEmitter>,
    parameter_inputs: Vec<String>,
    slot_count: usize,
}

impl CompiledParticleEffect {
    /// The normalized document; an owned copy independent of the caller's source JSON.
    pub fn asset(&self) -> &ParticleEffectAsset {
        &self.inner.asset
    }

    /// Compiled emitters, in document order.
    pub fn emitters(&self) -> &[CompiledEmitter] {
        &self.inner.emitters
    }

    /// Index of the emitter with `id`.
    pub fn emitter_index(&self, id: &str) -> Option<usize> {
        self.inner.emitters.iter().position(|emitter| emitter.asset.id == id)
    }

    /// Flattened parameter component names in script storage order (`speed`, `windX`, `tint.r`, …).
    pub fn parameter_inputs(&self) -> &[String] {
        &self.inner.parameter_inputs
    }

    /// Whether any emitter draws with a textured or flipbook renderer.
    pub fn is_textured(&self) -> bool {
        self.emitters().iter().any(|emitter| emitter.asset.renderer.is_textured())
    }

    /// Whether two handles share one definition.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Scratch slots needed to run any of this effect's programs.
    pub(crate) fn slot_count(&self) -> usize {
        self.inner.slot_count
    }
}

/// A `.joyfx` document failed validation or script compilation.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ParticleAssetError {
    /// The text was not JSON.
    #[error("Invalid particle effect JSON: {0}")]
    Json(String),
    /// A document field was missing, mistyped, or out of range; the message matches the TypeScript engine's.
    #[error("{0}")]
    Invalid(String),
    /// An embedded texture source was invalid.
    #[error(transparent)]
    Texture(#[from] ParticleTextureError),
    /// An emitter script failed to compile; the script error keeps its line and column.
    #[error("Emitter '{emitter}': {source}")]
    Script {
        /// The emitter whose spawn or update script failed.
        emitter: String,
        /// The located diagnostic.
        source: ParticleScriptError,
    },
}

/// Parse `.joyfx` text and compile it; see [`compile_particle_effect`].
pub fn parse_particle_effect(text: &str) -> Result<CompiledParticleEffect, ParticleAssetError> {
    let value: Value = serde_json::from_str(text).map_err(|error| ParticleAssetError::Json(error.to_string()))?;
    compile_particle_effect(&value)
}

/// Validate and compile a JSON effect document into an immutable shared definition.
///
/// The definition owns a normalized copy, so callers may keep editing their source JSON. Validation order and messages
/// match the TypeScript `compileParticleEffect`; script failures are reported as [`ParticleAssetError::Script`] with the
/// emitter id and source location.
pub fn compile_particle_effect(input: &Value) -> Result<CompiledParticleEffect, ParticleAssetError> {
    let source = record(Some(input), "effect")?;
    if source.get("version").and_then(Value::as_f64) != Some(f64::from(PARTICLE_EFFECT_VERSION)) {
        return Err(invalid("Unsupported particle effect version; expected 1."));
    }
    let name = label(source.get("name"), "effect.name")?.to_owned();
    let max_particles = number(source.get("maxParticles"), "maxParticles", 1.0, 4096.0, true)? as u32;
    let supplied_parameters = record(source.get("parameters"), "parameters")?;
    if supplied_parameters.len() > MAXIMUM_PARAMETERS {
        return Err(invalid("An effect supports at most 32 parameters."));
    }

    // Parameters, then their flattened script inputs and named containers.
    let mut parameters = IndexMap::new();
    for (key, value) in supplied_parameters {
        identifier(Some(&Value::from(key.as_str())), "parameter")?;
        if is_reserved(key) {
            return Err(invalid(format!("Reserved parameter name '{key}'.")));
        }
        let parameter = record(Some(value), &format!("parameter {key}"))?;
        parameters.insert(key.clone(), read_parameter(parameter, key)?);
    }
    let inputs: Vec<String> = parameters.iter().flat_map(|(name, parameter)| parameter.input_names(name)).collect();
    let containers: Vec<(String, ParticleValueKind)> = parameters
        .iter()
        .filter_map(|(name, parameter)| match parameter {
            ParticleParameter::Vector3 { .. } => Some((name.clone(), ParticleValueKind::Vector)),
            ParticleParameter::Color { .. } => Some((name.clone(), ParticleValueKind::Color)),
            ParticleParameter::Scalar { .. } => None,
        })
        .collect();
    let unique_inputs: std::collections::HashSet<&str> = inputs.iter().map(String::as_str).collect();
    if unique_inputs.len() != inputs.len()
        || inputs.iter().any(|input| is_reserved(input))
        || containers.iter().any(|(name, _)| inputs.contains(name))
    {
        return Err(invalid("Scalar and vector parameter component names must be unique and unreserved."));
    }

    // Emitters.
    let authored = match source.get("emitters") {
        Some(Value::Array(emitters)) if (1..=MAXIMUM_EMITTERS).contains(&emitters.len()) => emitters,
        _ => return Err(invalid("An effect requires 1–32 emitters.")),
    };
    let mut emitters: Vec<ParticleEmitterAsset> = Vec::with_capacity(authored.len());
    for (index, value) in authored.iter().enumerate() {
        let emitter = read_emitter(value, index)?;
        if emitters.iter().any(|existing| existing.id == emitter.id) {
            return Err(invalid(format!("Duplicate emitter '{}'.", emitter.id)));
        }
        emitters.push(emitter);
    }
    let texture_sources: std::collections::HashSet<&str> =
        emitters.iter().filter_map(|emitter| emitter.texture.as_ref().map(|texture| texture.source.as_ref())).collect();
    let texture_text: usize =
        emitters.iter().filter_map(|emitter| emitter.texture.as_ref()).map(|texture| texture.source.len()).sum();
    if texture_sources.len() > MAXIMUM_TEXTURE_SOURCES || texture_text > MAXIMUM_TEXTURE_TEXT {
        return Err(invalid("An effect supports at most 8 distinct PNG sources and 4 MiB of embedded texture data."));
    }
    let index_of = |id: &str| emitters.iter().position(|emitter| emitter.id == id);
    for emitter in &emitters {
        let children =
            [emitter.trail.as_ref().map(|trail| &trail.emitter), emitter.death.as_ref().map(|death| &death.emitter)];
        for child in children.into_iter().flatten() {
            if index_of(child).is_none() {
                return Err(invalid(format!("Emitter '{}' references missing child '{child}'.", emitter.id)));
            }
        }
    }

    // Scripts.
    let input_names: Vec<&str> = inputs.iter().map(String::as_str).collect();
    let container_names: Vec<(&str, ParticleValueKind)> =
        containers.iter().map(|(name, kind)| (name.as_str(), *kind)).collect();
    let mut compiled = Vec::with_capacity(emitters.len());
    for emitter in &emitters {
        let script_error = |source| ParticleAssetError::Script { emitter: emitter.id.clone(), source };
        let texture_dimensions = match &emitter.texture {
            Some(texture) => Some(read_particle_texture_source(&texture.source)?),
            None => None,
        };
        let spawn_program =
            compile_particle_script(&emitter.spawn, &input_names, &container_names).map_err(script_error)?;
        let update_program =
            compile_particle_script(&emitter.update, &input_names, &container_names).map_err(script_error)?;
        compiled.push(CompiledEmitter {
            asset: emitter.clone(),
            texture_dimensions,
            spawn_program,
            update_program,
            trail_index: emitter.trail.as_ref().and_then(|trail| index_of(&trail.emitter)),
            death_index: emitter.death.as_ref().and_then(|death| index_of(&death.emitter)),
        });
    }

    let slot_count = compiled
        .iter()
        .flat_map(|emitter| [emitter.spawn_program.slot_count(), emitter.update_program.slot_count()])
        .fold(FIXED_SLOT_COUNT, usize::max);
    let asset = ParticleEffectAsset { version: PARTICLE_EFFECT_VERSION, name, max_particles, parameters, emitters };
    Ok(CompiledParticleEffect {
        inner: Arc::new(CompiledEffectInner { asset, emitters: compiled, parameter_inputs: inputs, slot_count }),
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// Document readers
// ---------------------------------------------------------------------------------------------------------------------

fn invalid(message: impl Into<String>) -> ParticleAssetError {
    ParticleAssetError::Invalid(message.into())
}

fn is_reserved(name: &str) -> bool {
    PARTICLE_FIELDS.contains(&name) || PARTICLE_INPUTS.contains(&name) || RESERVED_NAMES.contains(&name)
}

fn read_parameter(parameter: &Map<String, Value>, key: &str) -> Result<ParticleParameter, ParticleAssetError> {
    let kind = parameter.get("type");
    if kind.and_then(Value::as_str) == Some(ParticleParameterType::Color.as_str()) {
        let min = components::<4>(parameter.get("min"), &format!("{key}.min"), &[0.0; 4], &COLOR_PARAMETER_LIMITS)?;
        let max = components::<4>(parameter.get("max"), &format!("{key}.max"), &min, &COLOR_PARAMETER_LIMITS)?;
        let value = components::<4>(parameter.get("value"), &format!("{key}.value"), &min, &max)?;
        return Ok(ParticleParameter::Color { value, min, max });
    }
    let is_vector = kind.and_then(Value::as_str) == Some(ParticleParameterType::Vector3.as_str());
    let is_scalar = kind.and_then(Value::as_str) == Some(ParticleParameterType::Scalar.as_str());
    if kind.is_some() && !is_vector && !is_scalar {
        return Err(invalid(format!("{key}.type must be scalar, vector3, or color.")));
    }
    if !is_vector {
        let limit = MAXIMUM_PARAMETER_MAGNITUDE;
        let min = number(parameter.get("min"), &format!("{key}.min"), -limit, limit, false)?;
        let max = number(parameter.get("max"), &format!("{key}.max"), min, limit, false)?;
        let value = number(parameter.get("value"), &format!("{key}.value"), min, max, false)?;
        return Ok(ParticleParameter::Scalar { value, min, max });
    }
    let lower = [-MAXIMUM_PARAMETER_MAGNITUDE; 3];
    let upper = [MAXIMUM_PARAMETER_MAGNITUDE; 3];
    let min = components::<3>(parameter.get("min"), &format!("{key}.min"), &lower, &upper)?;
    let max = components::<3>(parameter.get("max"), &format!("{key}.max"), &min, &upper)?;
    let value = components::<3>(parameter.get("value"), &format!("{key}.value"), &min, &max)?;
    Ok(ParticleParameter::Vector3 { value, min, max })
}

/// Read an `N`-component array with per-component bounds (`must be an XYZ array` or `an RGBA array`).
fn components<const N: usize>(
    value: Option<&Value>,
    path: &str,
    min: &[f64; N],
    max: &[f64; N],
) -> Result<[f64; N], ParticleAssetError> {
    let shape = if N == 3 { "an XYZ array" } else { "an RGBA array" };
    let Some(Value::Array(items)) = value.filter(|value| value.as_array().is_some_and(|items| items.len() == N)) else {
        return Err(invalid(format!("{path} must be {shape}.")));
    };
    let mut result = [0.0; N];
    for (index, slot) in result.iter_mut().enumerate() {
        *slot = number(items.get(index), &format!("{path}[{index}]"), min[index], max[index], false)?;
    }
    Ok(result)
}

fn read_emitter(value: &Value, index: usize) -> Result<ParticleEmitterAsset, ParticleAssetError> {
    let item = record(Some(value), &format!("emitters[{index}]"))?;
    let id = identifier(item.get("id"), &format!("emitters[{index}].id"))?.to_owned();
    let (Some(Value::Bool(enabled)), Some(Value::Bool(root))) = (item.get("enabled"), item.get("root")) else {
        return Err(invalid(format!("{id}: enabled and root must be booleans.")));
    };
    let duration = number(item.get("duration"), &format!("{id}.duration"), 0.0, 30.0, false)?;
    let rate = number(item.get("rate"), &format!("{id}.rate"), 0.0, 2000.0, false)?;
    let lifetime = number(item.get("lifetime"), &format!("{id}.lifetime"), 1.0 / 600.0, 30.0, false)?;
    let authored_bursts = match item.get("bursts") {
        Some(Value::Array(bursts)) if bursts.len() <= MAXIMUM_BURSTS => bursts,
        _ => return Err(invalid(format!("{id}: bursts must be an array of at most 128 entries."))),
    };
    let mut bursts = Vec::with_capacity(authored_bursts.len());
    for (burst_index, value) in authored_bursts.iter().enumerate() {
        let burst = record(Some(value), &format!("{id}.bursts[{burst_index}]"))?;
        let time = number(burst.get("time"), &format!("{id}.burst.time"), 0.0, duration, false)?;
        let count = number(burst.get("count"), &format!("{id}.burst.count"), 1.0, 4096.0, true)? as u32;
        bursts.push(ParticleBurst { time, count });
    }
    bursts.sort_by(|left, right| left.time.total_cmp(&right.time));

    let renderer = item
        .get("renderer")
        .and_then(Value::as_str)
        .and_then(ParticleRenderer::parse)
        .ok_or_else(|| invalid(format!("{id}: unknown particle renderer.")))?;
    let (Some(Value::String(spawn)), Some(Value::String(update))) = (item.get("spawn"), item.get("update")) else {
        return Err(invalid(format!("{id}: spawn and update scripts must be strings.")));
    };

    let light = match item.get("light") {
        None => None,
        Some(value) => {
            let light = record(Some(value), &format!("{id}.light"))?;
            let casts_shadow = match light.get(PARTICLE_LIGHT_CASTS_SHADOW) {
                None => false,
                Some(Value::Bool(casts_shadow)) => *casts_shadow,
                Some(_) => return Err(invalid(format!("{id}.light.{PARTICLE_LIGHT_CASTS_SHADOW} must be a boolean."))),
            };
            Some(ParticleLight {
                intensity: number(light.get("intensity"), &format!("{id}.light.intensity"), 0.0, 1000.0, false)?,
                range: number(light.get("range"), &format!("{id}.light.range"), 0.01, 10000.0, false)?,
                casts_shadow,
            })
        }
    };
    let texture = match item.get("texture") {
        Some(value) => Some(read_texture(value, &id)?),
        None if renderer.is_textured() => {
            return Err(invalid(format!("{id}: {} requires a texture.", renderer.as_str())));
        }
        None => None,
    };
    let trail = match item.get("trail") {
        None => None,
        Some(value) => {
            let path = format!("{id}.trail");
            let trail = record(Some(value), &path)?;
            let (emitter, count) = child(trail, &path)?;
            let interval = number(trail.get("interval"), &format!("{path}.interval"), 1.0 / 60.0, 30.0, false)?;
            Some(ParticleTrail { emitter, count, interval })
        }
    };
    let death = match item.get("death") {
        None => None,
        Some(value) => {
            let path = format!("{id}.death");
            let (emitter, count) = child(record(Some(value), &path)?, &path)?;
            Some(ParticleChild { emitter, count })
        }
    };

    Ok(ParticleEmitterAsset {
        id,
        enabled: *enabled,
        root: *root,
        duration,
        rate,
        bursts,
        lifetime,
        renderer,
        texture,
        light,
        spawn: spawn.clone(),
        update: update.clone(),
        trail,
        death,
    })
}

fn read_texture(value: &Value, id: &str) -> Result<ParticleTexture, ParticleAssetError> {
    let item = record(Some(value), &format!("{id}.texture"))?;
    let source = match item.get("source") {
        Some(Value::String(source)) => source.as_str(),
        _ => return Err(ParticleTextureError::NotDataUrl.into()),
    };
    let dimensions = read_particle_texture_source(source)?;
    let optional = |key: &str, default: f64| item.get(key).cloned().unwrap_or(Value::from(default));

    let column_limit = f64::from(dimensions.width.min(64));
    let row_limit = f64::from(dimensions.height.min(64));
    let columns = number(Some(&optional("columns", 1.0)), &format!("{id}.texture.columns"), 1.0, column_limit, true)?;
    let rows = number(Some(&optional("rows", 1.0)), &format!("{id}.texture.rows"), 1.0, row_limit, true)?;
    let cells = columns * rows;
    let frames = number(Some(&optional("frames", cells)), &format!("{id}.texture.frames"), 1.0, cells, true)?;
    let fps = number(Some(&optional("fps", 0.0)), &format!("{id}.texture.fps"), 0.0, 120.0, false)?;
    let looping = match item.get("loop") {
        None => true,
        Some(Value::Bool(looping)) => *looping,
        Some(_) => return Err(invalid(format!("{id}.texture.loop must be a boolean."))),
    };
    Ok(ParticleTexture {
        source: Arc::from(source),
        columns: columns as u32,
        rows: rows as u32,
        frames: frames as u32,
        fps,
        looping,
    })
}

fn child(source: &Map<String, Value>, path: &str) -> Result<(String, u32), ParticleAssetError> {
    let emitter = identifier(source.get("emitter"), &format!("{path}.emitter"))?.to_owned();
    let count = number(source.get("count"), &format!("{path}.count"), 1.0, 4096.0, true)? as u32;
    Ok((emitter, count))
}

fn record<'v>(value: Option<&'v Value>, path: &str) -> Result<&'v Map<String, Value>, ParticleAssetError> {
    match value {
        Some(Value::Object(record)) => Ok(record),
        _ => Err(invalid(format!("{path} must be an object."))),
    }
}

fn label<'v>(value: Option<&'v Value>, path: &str) -> Result<&'v str, ParticleAssetError> {
    match value {
        Some(Value::String(text)) if !text.trim().is_empty() && text.chars().count() <= 80 => Ok(text),
        _ => Err(invalid(format!("{path} requires 1–80 characters."))),
    }
}

fn identifier<'v>(value: Option<&'v Value>, path: &str) -> Result<&'v str, ParticleAssetError> {
    let name = label(value, path)?;
    let mut characters = name.chars();
    let valid = characters.next().is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
        && name.len() <= 48;
    if !valid {
        return Err(invalid(format!("{path} must be a simple identifier of at most 48 characters.")));
    }
    Ok(name)
}

fn number(value: Option<&Value>, path: &str, min: f64, max: f64, integer: bool) -> Result<f64, ParticleAssetError> {
    let number = value
        .and_then(Value::as_f64)
        .filter(|number| number.is_finite() && *number >= min && *number <= max && (!integer || number.fract() == 0.0));
    number.ok_or_else(|| {
        let kind = if integer { "an integer" } else { "a number" };
        invalid(format!("{path} must be {kind} from {} to {}.", format_number(min), format_number(max)))
    })
}

/// A number as `JSON.stringify` writes it: integral values without a fractional part.
fn json_number(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
        return Value::from(value as i64);
    }
    Value::from(value)
}

fn json_numbers(values: &[f64]) -> Value {
    Value::Array(values.iter().copied().map(json_number).collect())
}

#[cfg(test)]
#[path = "asset.test.rs"]
mod tests;
