// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The `.joyfx` working document: portable
//! reads that the engine compiler fully validates, lenient recovery reads that keep unfinished drafts, export text,
//! property snapshots for undo, and the structural edits the inspector makes (emitters, parameters, bursts, children,
//! textures, and lights).
//!
//! The working document is the engine's [`ParticleEffectAsset`]. It may hold a draft the compiler rejects (an
//! unfinished script, a missing child target); only an applied definition reaches the preview. Nothing here touches
//! the interface.

use joy_engine::particles::{
    DEFAULT_PARTICLE_TEXTURE,
    PARTICLE_EFFECT_VERSION,
    PARTICLE_LIGHT_CASTS_SHADOW,
    ParticleBurst,
    ParticleChild,
    ParticleEffectAsset,
    ParticleEmitterAsset,
    ParticleLight,
    ParticleParameter,
    ParticleRenderer,
    ParticleTexture,
    ParticleTrail,
    parse_particle_effect,
};
use joy_engine::serde_json::{
    self,
    Map,
    Value,
};

use crate::constants::EmitterPreset;
use crate::examples::create_emitter_template;

/// Largest portable asset or recovery draft, in bytes.
pub const MAX_ASSET_BYTES: usize = 6_000_000;

/// Largest magnitude of a scalar or vector parameter bound.
pub const MAXIMUM_PARAMETER_MAGNITUDE: f64 = 1e6;

/// Parse and fully validate a portable asset; the shared compiler is the authority for every nested emitter and
/// script. Returns the normalized document.
pub fn read_asset(text: &str) -> Result<ParticleEffectAsset, String> {
    if text.len() > MAX_ASSET_BYTES {
        return Err(format!("Particle assets cannot exceed {} bytes.", group_thousands(MAX_ASSET_BYTES)));
    }
    parse_particle_effect(text).map(|compiled| compiled.asset().clone()).map_err(|error| error.to_string())
}

/// Restore a local working draft without compiling unfinished scripts. Portable file imports go through
/// [`read_asset`]; recovery deliberately keeps text that may be only half of the author's next valid expression, and
/// fills missing or mistyped fields with template defaults.
pub fn read_recovery_asset(text: &str) -> Result<ParticleEffectAsset, String> {
    if text.len() > MAX_ASSET_BYTES {
        return Err(format!("Particle recovery cannot exceed {} bytes.", group_thousands(MAX_ASSET_BYTES)));
    }
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("Invalid particle recovery JSON: {error}"))?;
    let Value::Object(record) = value else {
        return Err("Particle recovery must contain an effect object.".to_owned());
    };
    Ok(lenient_asset(&record))
}

/// Export text: two-space indented JSON with a trailing newline.
pub fn write_asset(asset: &ParticleEffectAsset) -> String {
    let text = serde_json::to_string_pretty(&asset.to_json()).expect("effect JSON always serializes");
    format!("{text}\n")
}

/// A file name stem from an effect name: lowercase words joined by hyphens, or `particle-effect`.
pub fn file_stem(name: &str) -> String {
    let mut stem = String::new();
    let mut pending_hyphen = false;
    for character in name.to_lowercase().chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            if pending_hyphen && !stem.is_empty() {
                stem.push('-');
            }
            pending_hyphen = false;
            stem.push(character);
        } else {
            pending_hyphen = true;
        }
    }
    if stem.is_empty() { "particle-effect".to_owned() } else { stem }
}

/// The export file name for an effect name.
pub fn export_file_name(name: &str) -> String {
    format!("{}.joyfx", file_stem(name))
}

// ---------------------------------------------------------------------------------------------------------------------
// Property snapshots
// ---------------------------------------------------------------------------------------------------------------------

/// The properties property undo covers: parameters and emitter settings, without scripts (script undo is separate)
/// or embedded texture bytes. Stored as an effect document whose name, capacity, scripts, and texture sources are
/// cleared, so snapshots compare only the covered properties.
#[derive(Clone, Debug, PartialEq)]
pub struct PropertySnapshot(ParticleEffectAsset);

/// Snapshot `asset`'s properties.
pub fn property_snapshot(asset: &ParticleEffectAsset) -> PropertySnapshot {
    let mut snapshot = asset.clone();
    snapshot.name.clear();
    snapshot.max_particles = 0;
    for emitter in &mut snapshot.emitters {
        emitter.spawn.clear();
        emitter.update.clear();
        if let Some(texture) = &mut emitter.texture {
            texture.source = "".into();
        }
    }
    PropertySnapshot(snapshot)
}

/// Restore snapshot properties into `asset`, keeping its scripts and texture sources by emitter position (a restored
/// texture without a current source uses the default texture).
pub fn restore_properties(asset: &mut ParticleEffectAsset, snapshot: &PropertySnapshot) {
    let current = std::mem::take(&mut asset.emitters);
    asset.parameters = snapshot.0.parameters.clone();
    asset.emitters = snapshot
        .0
        .emitters
        .iter()
        .enumerate()
        .map(|(index, properties)| {
            let mut emitter = properties.clone();
            let existing = current.get(index);
            emitter.spawn = existing.map(|emitter| emitter.spawn.clone()).unwrap_or_default();
            emitter.update = existing.map(|emitter| emitter.update.clone()).unwrap_or_default();
            if let Some(texture) = &mut emitter.texture {
                texture.source = existing
                    .and_then(|emitter| emitter.texture.as_ref())
                    .map_or_else(|| DEFAULT_PARTICLE_TEXTURE.into(), |texture| texture.source.clone());
            }
            emitter
        })
        .collect();
}

// ---------------------------------------------------------------------------------------------------------------------
// Parameters
// ---------------------------------------------------------------------------------------------------------------------

/// Which bound of a parameter component an edit changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bound {
    /// The lower bound.
    Minimum,
    /// The upper bound.
    Maximum,
}

/// The kind of a new or converted parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParameterKind {
    /// One number.
    Scalar,
    /// XYZ components.
    Vector,
    /// RGBA components.
    Color,
}

impl ParameterKind {
    /// The kind of `parameter`.
    pub fn of(parameter: &ParticleParameter) -> Self {
        match parameter {
            ParticleParameter::Scalar {
                ..
            } => Self::Scalar,
            ParticleParameter::Vector3 {
                ..
            } => Self::Vector,
            ParticleParameter::Color {
                ..
            } => Self::Color,
        }
    }

    /// Menu label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Scalar => "Scalar",
            Self::Vector => "Vector",
            Self::Color => "Color",
        }
    }

    /// One-letter badge (`S`, `V`, `C`).
    pub fn code(self) -> &'static str {
        match self {
            Self::Scalar => "S",
            Self::Vector => "V",
            Self::Color => "C",
        }
    }

    /// The template a parameter of this kind starts from.
    pub fn template(self) -> ParticleParameter {
        match self {
            Self::Scalar => ParticleParameter::Scalar {
                value: 1.0,
                min: 0.0,
                max: 2.0,
            },
            Self::Vector => ParticleParameter::Vector3 {
                value: [1.0; 3],
                min: [0.0; 3],
                max: [2.0; 3],
            },
            Self::Color => ParticleParameter::Color {
                value: [1.0; 4],
                min: [0.0; 4],
                max: [1.0; 4],
            },
        }
    }
}

/// Mutable views of a parameter's value, minimum, and maximum components.
pub fn parameter_components(parameter: &mut ParticleParameter) -> (&mut [f64], &mut [f64], &mut [f64]) {
    match parameter {
        ParticleParameter::Scalar {
            value,
            min,
            max,
        } => (std::slice::from_mut(value), std::slice::from_mut(min), std::slice::from_mut(max)),
        ParticleParameter::Vector3 {
            value,
            min,
            max,
        } => (value, min, max),
        ParticleParameter::Color {
            value,
            min,
            max,
        } => (value, min, max),
    }
}

/// The limits a bound edit of `component` must respect: color RGB is nonnegative linear light (HDR above one) and
/// color alpha stays in 0–1; other parameters stay within ±1e6.
pub fn bound_limits(parameter: &ParticleParameter, component: usize) -> (f64, f64) {
    match parameter {
        ParticleParameter::Color {
            ..
        } if component == COLOR_ALPHA_COMPONENT => (0.0, 1.0),
        ParticleParameter::Color {
            ..
        } => (0.0, MAXIMUM_PARAMETER_MAGNITUDE),
        _ => (-MAXIMUM_PARAMETER_MAGNITUDE, MAXIMUM_PARAMETER_MAGNITUDE),
    }
}

/// Index of a color parameter's alpha component.
const COLOR_ALPHA_COMPONENT: usize = 3;

/// Whether any script reads parameter `name` (or, for vectors, its `nameX`/`nameY`/`nameZ` components), as a whole
/// identifier.
pub fn parameter_is_used(asset: &ParticleEffectAsset, name: &str) -> bool {
    let mut names = vec![name.to_owned()];
    if matches!(asset.parameters.get(name), Some(ParticleParameter::Vector3 { .. })) {
        names.extend(["X", "Y", "Z"].iter().map(|axis| format!("{name}{axis}")));
    }
    asset.emitters.iter().any(|emitter| {
        names.iter().any(|name| contains_identifier(&emitter.spawn, name) || contains_identifier(&emitter.update, name))
    })
}

/// Add `parameter1`, `parameter2`, … (the first free name) as a scalar; returns the name.
pub fn add_parameter(asset: &mut ParticleEffectAsset) -> String {
    let mut index = 1;
    while asset.parameters.contains_key(&format!("parameter{index}")) {
        index += 1;
    }
    let name = format!("parameter{index}");
    asset.parameters.insert(name.clone(), ParameterKind::Scalar.template());
    name
}

/// Delete parameter `name` unless a script uses it.
pub fn delete_parameter(asset: &mut ParticleEffectAsset, name: &str) -> Result<(), String> {
    if parameter_is_used(asset, name) {
        return Err(format!("Parameter '{name}' is used by a script and cannot be deleted."));
    }
    asset.parameters.shift_remove(name);
    Ok(())
}

/// Rename parameter `name` to the trimmed `next`, which moves the parameter to the end of the document order. Returns
/// the new name, or `None` when the name is empty or unchanged.
pub fn rename_parameter(asset: &mut ParticleEffectAsset, name: &str, next: &str) -> Result<Option<String>, String> {
    let next = next.trim();
    if next.is_empty() || next == name {
        return Ok(None);
    }
    if asset.parameters.contains_key(next) {
        return Err(format!("Parameter '{next}' already exists."));
    }
    if parameter_is_used(asset, name) {
        return Err(format!("Parameter '{name}' is used by a script and cannot be renamed."));
    }
    let Some(parameter) = asset.parameters.shift_remove(name) else {
        return Ok(None);
    };
    asset.parameters.insert(next.to_owned(), parameter);
    Ok(Some(next.to_owned()))
}

/// Replace parameter `name` with a template of `kind`, unless a script uses it.
pub fn change_parameter_kind(asset: &mut ParticleEffectAsset, name: &str, kind: ParameterKind) -> Result<(), String> {
    let Some(parameter) = asset.parameters.get(name) else {
        return Ok(());
    };
    if ParameterKind::of(parameter) == kind {
        return Ok(());
    }
    if parameter_is_used(asset, name) {
        return Err(format!("Parameter '{name}' is used by a script; remove its references before changing type."));
    }
    asset.parameters.insert(name.to_owned(), kind.template());
    Ok(())
}

/// Validate a bound edit: finite, within the parameter's limits, and keeping minimum no greater than maximum.
pub fn check_bound(parameter: &ParticleParameter, bound: Bound, component: usize, value: f64) -> Result<(), String> {
    let mut copy = parameter.clone();
    let (_, minimums, maximums) = parameter_components(&mut copy);
    let (lower, upper) = bound_limits(parameter, component);
    let minimum = minimums.get(component).copied().unwrap_or(lower);
    let maximum = maximums.get(component).copied().unwrap_or(upper);
    let field = if bound == Bound::Minimum { "min" } else { "max" };
    if !value.is_finite()
        || value < lower
        || value > upper
        || (bound == Bound::Minimum && value > maximum)
        || (bound == Bound::Maximum && value < minimum)
    {
        return Err(format!(
            "Enter a finite {field} within {}–{}, with minimum no greater than maximum.",
            joy_engine::development::lab_values::format_number(lower),
            joy_engine::development::lab_values::format_number(upper)
        ));
    }
    Ok(())
}

/// Clamp each value component into its (possibly tightened) bounds, after a bound edit.
pub fn clamp_parameter_value(parameter: &mut ParticleParameter) {
    let (values, minimums, maximums) = parameter_components(parameter);
    for ((value, minimum), maximum) in values.iter_mut().zip(minimums.iter()).zip(maximums.iter()) {
        *value = value.min(*maximum).max(*minimum);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Emitters
// ---------------------------------------------------------------------------------------------------------------------

/// A unique emitter id from `base`: non-identifier characters become `_`, then `_2`, `_3`, … until unused.
pub fn unique_emitter_id(asset: &ParticleEffectAsset, base: &str) -> String {
    let sanitize = |text: &str| -> String {
        text.chars()
            .map(|character| if character.is_ascii_alphanumeric() || character == '_' { character } else { '_' })
            .collect()
    };
    let mut id = sanitize(base);
    let mut index = 2;
    while asset.emitters.iter().any(|emitter| emitter.id == id) {
        id = sanitize(&format!("{base}_{index}"));
        index += 1;
    }
    id
}

/// Append an emitter from `preset` with a unique id; returns its index.
pub fn add_emitter(asset: &mut ParticleEffectAsset, preset: EmitterPreset) -> usize {
    let id = unique_emitter_id(asset, "emitter");
    asset.emitters.push(create_emitter_template(&id, preset));
    asset.emitters.len() - 1
}

/// Append a root copy of emitter `index` with a unique `<id>_copy` id; returns the copy's index.
pub fn duplicate_emitter(asset: &mut ParticleEffectAsset, index: usize) -> Option<usize> {
    let mut copy = asset.emitters.get(index)?.clone();
    copy.id = unique_emitter_id(asset, &format!("{}_copy", copy.id));
    copy.root = true;
    asset.emitters.push(copy);
    Some(asset.emitters.len() - 1)
}

/// Delete emitter `index` (an asset keeps at least one), dropping trail and death links to it; returns the index to
/// select next.
pub fn delete_emitter(asset: &mut ParticleEffectAsset, index: usize) -> Result<usize, String> {
    if asset.emitters.len() <= 1 {
        return Err("An asset needs at least one emitter.".to_owned());
    }
    if index >= asset.emitters.len() {
        return Ok(asset.emitters.len() - 1);
    }
    let removed = asset.emitters.remove(index).id;
    for emitter in &mut asset.emitters {
        if emitter.trail.as_ref().is_some_and(|trail| trail.emitter == removed) {
            emitter.trail = None;
        }
        if emitter.death.as_ref().is_some_and(|death| death.emitter == removed) {
            emitter.death = None;
        }
    }
    Ok(index.saturating_sub(1))
}

/// Rename emitter `index` to `next`, updating trail and death links; duplicate ids are refused.
pub fn rename_emitter(asset: &mut ParticleEffectAsset, index: usize, next: &str) -> Result<(), String> {
    let Some(previous) = asset.emitters.get(index).map(|emitter| emitter.id.clone()) else {
        return Ok(());
    };
    if next != previous && asset.emitters.iter().any(|emitter| emitter.id == next) {
        return Err(format!("Emitter '{next}' already exists."));
    }
    asset.emitters[index].id = next.to_owned();
    for emitter in &mut asset.emitters {
        if let Some(trail) = &mut emitter.trail
            && trail.emitter == previous
        {
            trail.emitter = next.to_owned();
        }
        if let Some(death) = &mut emitter.death
            && death.emitter == previous
        {
            death.emitter = next.to_owned();
        }
    }
    Ok(())
}

/// Switch emitter `index`'s renderer; textured renderers without a texture get the default texture.
pub fn set_renderer(emitter: &mut ParticleEmitterAsset, renderer: ParticleRenderer) {
    emitter.renderer = renderer;
    if renderer.is_textured() && emitter.texture.is_none() {
        emitter.texture = Some(default_texture(DEFAULT_PARTICLE_TEXTURE));
    }
}

/// A texture record for `source` with a single frame at 12 frames per second, looping.
pub fn default_texture(source: &str) -> ParticleTexture {
    ParticleTexture {
        source: source.into(),
        columns: 1,
        rows: 1,
        frames: 1,
        fps: 12.0,
        looping: true,
    }
}

/// Turn particle light emission on (intensity 2, range 220, no shadows) or off.
pub fn set_light_enabled(emitter: &mut ParticleEmitterAsset, enabled: bool) {
    emitter.light = enabled.then_some(ParticleLight {
        intensity: 2.0,
        range: 220.0,
        casts_shadow: false,
    });
}

/// Which child link an edit changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChildLink {
    /// Particles spawned along a particle's life.
    Trail,
    /// Particles spawned when a particle expires.
    Death,
}

/// Point a child link at `target` (`None` removes it); a new link starts at count 1 (and a 0.1 s trail interval).
pub fn set_child_target(emitter: &mut ParticleEmitterAsset, link: ChildLink, target: Option<&str>) {
    match (link, target) {
        (ChildLink::Trail, None) => emitter.trail = None,
        (ChildLink::Death, None) => emitter.death = None,
        (ChildLink::Trail, Some(target)) => {
            emitter.trail = Some(ParticleTrail {
                emitter: target.to_owned(),
                count: 1,
                interval: 0.1,
            });
        }
        (ChildLink::Death, Some(target)) => {
            emitter.death = Some(ParticleChild {
                emitter: target.to_owned(),
                count: 1,
            })
        }
    }
}

/// Append a burst of ten particles at time zero.
pub fn add_burst(emitter: &mut ParticleEmitterAsset) {
    emitter.bursts.push(ParticleBurst {
        time: 0.0,
        count: 10,
    });
}

/// The preview timeline length in seconds. Child chains and scripts can extend well beyond a root emitter's own
/// timing, so the format's hard seek horizon exposes every valid effect without guessing.
pub fn calculate_duration(_asset: &ParticleEffectAsset) -> f64 {
    30.0
}

// ---------------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------------

/// Whether `text` contains `name` delimited by non-identifier characters.
fn contains_identifier(text: &str, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let identifier = |character: char| character.is_ascii_alphanumeric() || character == '_';
    text.match_indices(name).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let after = text[start + name.len()..].chars().next();
        !before.is_some_and(identifier) && !after.is_some_and(identifier)
    })
}

/// `6000000` as `6,000,000`: English thousands separators.
fn group_thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// Read a recovery record field by field, falling back to template values for missing or mistyped fields.
fn lenient_asset(record: &Map<String, Value>) -> ParticleEffectAsset {
    let text = |value: Option<&Value>, fallback: &str| value.and_then(Value::as_str).unwrap_or(fallback).to_owned();
    let mut asset = ParticleEffectAsset {
        version: PARTICLE_EFFECT_VERSION,
        name: text(record.get("name"), "Particle Effect"),
        max_particles: record
            .get("maxParticles")
            .and_then(Value::as_f64)
            .map_or(1024, |value| value.clamp(1.0, 4096.0) as u32),
        parameters: Default::default(),
        emitters: Vec::new(),
    };
    if let Some(Value::Object(parameters)) = record.get("parameters") {
        for (name, value) in parameters {
            if let Some(parameter) = lenient_parameter(value) {
                asset.parameters.insert(name.clone(), parameter);
            }
        }
    }
    if let Some(Value::Array(emitters)) = record.get("emitters") {
        for (index, value) in emitters.iter().enumerate() {
            if let Value::Object(emitter) = value {
                asset.emitters.push(lenient_emitter(emitter, index));
            }
        }
    }
    if asset.emitters.is_empty() {
        asset.emitters.push(create_emitter_template("emitter", EmitterPreset::Spray));
    }
    asset
}

fn lenient_parameter(value: &Value) -> Option<ParticleParameter> {
    let record = value.as_object()?;
    let numbers = |key: &str, count: usize| -> Option<Vec<f64>> {
        let items = record.get(key)?.as_array()?;
        let numbers: Option<Vec<f64>> = items.iter().map(Value::as_f64).collect();
        numbers.filter(|numbers| numbers.len() == count)
    };
    match record.get("type").and_then(Value::as_str) {
        Some("vector3") => {
            let array = |key: &str| numbers(key, 3).map(|values| [values[0], values[1], values[2]]);
            Some(ParticleParameter::Vector3 {
                value: array("value")?,
                min: array("min")?,
                max: array("max")?,
            })
        }
        Some("color") => {
            let array = |key: &str| numbers(key, 4).map(|values| [values[0], values[1], values[2], values[3]]);
            Some(ParticleParameter::Color {
                value: array("value")?,
                min: array("min")?,
                max: array("max")?,
            })
        }
        _ => {
            let number = |key: &str| record.get(key).and_then(Value::as_f64);
            Some(ParticleParameter::Scalar {
                value: number("value")?,
                min: number("min")?,
                max: number("max")?,
            })
        }
    }
}

fn lenient_emitter(record: &Map<String, Value>, index: usize) -> ParticleEmitterAsset {
    let template = create_emitter_template(&format!("emitter_{}", index + 1), EmitterPreset::Spray);
    let number = |key: &str, fallback: f64| record.get(key).and_then(Value::as_f64).unwrap_or(fallback);
    let flag = |key: &str, fallback: bool| record.get(key).and_then(Value::as_bool).unwrap_or(fallback);
    let text = |key: &str, fallback: &str| record.get(key).and_then(Value::as_str).unwrap_or(fallback).to_owned();
    let count =
        |value: Option<&Value>| value.and_then(Value::as_f64).map_or(1, |count| count.clamp(1.0, 4096.0) as u32);
    let bursts = record
        .get("bursts")
        .and_then(Value::as_array)
        .map(|bursts| {
            bursts
                .iter()
                .filter_map(Value::as_object)
                .map(|burst| ParticleBurst {
                    time: burst.get("time").and_then(Value::as_f64).unwrap_or(0.0),
                    count: count(burst.get("count")),
                })
                .collect()
        })
        .unwrap_or_default();
    let renderer =
        record.get("renderer").and_then(Value::as_str).and_then(ParticleRenderer::parse).unwrap_or(template.renderer);
    let texture = record.get("texture").and_then(Value::as_object).map(|texture| ParticleTexture {
        source: texture.get("source").and_then(Value::as_str).unwrap_or(DEFAULT_PARTICLE_TEXTURE).into(),
        columns: count(texture.get("columns")).min(64),
        rows: count(texture.get("rows")).min(64),
        frames: count(texture.get("frames")),
        fps: texture.get("fps").and_then(Value::as_f64).unwrap_or(12.0),
        looping: texture.get("loop").and_then(Value::as_bool).unwrap_or(true),
    });
    let light = record.get("light").and_then(Value::as_object).map(|light| ParticleLight {
        intensity: light.get("intensity").and_then(Value::as_f64).unwrap_or(2.0),
        range: light.get("range").and_then(Value::as_f64).unwrap_or(220.0),
        casts_shadow: light.get(PARTICLE_LIGHT_CASTS_SHADOW).and_then(Value::as_bool).unwrap_or(false),
    });
    let trail = record.get("trail").and_then(Value::as_object).and_then(|trail| {
        Some(ParticleTrail {
            emitter: trail.get("emitter")?.as_str()?.to_owned(),
            count: count(trail.get("count")),
            interval: trail.get("interval").and_then(Value::as_f64).unwrap_or(0.1),
        })
    });
    let death = record.get("death").and_then(Value::as_object).and_then(|death| {
        Some(ParticleChild {
            emitter: death.get("emitter")?.as_str()?.to_owned(),
            count: count(death.get("count")),
        })
    });
    let mut emitter = ParticleEmitterAsset {
        id: text("id", &template.id),
        enabled: flag("enabled", true),
        root: flag("root", true),
        duration: number("duration", template.duration),
        rate: number("rate", template.rate),
        bursts,
        lifetime: number("lifetime", template.lifetime),
        renderer,
        texture,
        light,
        spawn: text("spawn", ""),
        update: text("update", ""),
        trail,
        death,
    };
    if emitter.renderer.is_textured() && emitter.texture.is_none() {
        emitter.texture = Some(default_texture(DEFAULT_PARTICLE_TEXTURE));
    }
    emitter
}

#[cfg(test)]
#[path = "document.test.rs"]
mod tests;
