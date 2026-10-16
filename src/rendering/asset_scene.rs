// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Authored asset presentation for levels, the editor, and previews: FORM meshes plus per-entity particle effects.
//!
//! The Rust port of the TypeScript `src/gameplay/asset-scene.ts`, placed in the rendering layer because it presents
//! meshes and effects (TypeScript recorded that as a layer violation). Entities' `render` components resolve FORM and
//! FORM LAB meshes through a [`FormAssetLibrary`] and draw through [`LevelScene`]; `effect` components play `.joyfx`
//! effects attached to the entity transform, with the component's `seed`, `loop`, `enabled`, and `parameters`.
//!
//! Synchronization is staged as in TypeScript: [`AssetScene::prepare`] reads, compiles, decodes, and starts every
//! resource without touching the displayed presentation, and [`AssetScene::commit`] swaps it in only if no newer
//! preparation (or [`cancel_pending`](AssetScene::cancel_pending)) superseded it. A failed preparation leaves the running
//! presentation, its effects, and its caches unchanged. Asset text always comes from the caller's `read` function (a
//! file system, a bundle, or text fetched by a browser), never from this module, so it works the same in browsers.
//!
//! Beyond TypeScript, textured and flipbook effects are supported (draw them with [`AssetScene::particle_batches`] and
//! [`AssetScene::transparent_draws`]), and particle lights are available in world space.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::Arc;

use crate::gameplay::{
    AssetPathError, ComponentError, EFFECT_COMPONENT_DEFAULTS, EntityDefinition, LevelData, LightComponent,
    ObjectError, RenderComponent, RenderSource, asset_path, component_type, resolve_level_objects,
};
use crate::particles::{
    CompiledParticleEffect, ParticleAssetError, ParticleBatch, ParticleCameraBasis, ParticleEffect,
    ParticleEffectError, ParticleEffectOptions, ParticleLightSelection, ParticleParameterValue, ParticleRenderError,
    ParticleTextureError, ParticleTextureSet, ParticleTriangle, batch_particle_triangles, collect_particle_lights,
    collect_particle_triangles, parse_particle_effect, particle_seed, rank_particle_lights, textured_sources,
};
use indexmap::IndexMap;
use serde_json::Value;

use crate::rendering::error::RenderError;
use crate::rendering::form_assets::FormAssetLibrary;
use crate::rendering::frame::{ColorVertex, MAX_POINT_LIGHTS, PointLight, TransparentDraw};
use crate::rendering::level_scene::{LevelScene, LevelSceneError};
use crate::rendering::mesh::MeshAsset;
use crate::rendering::scene::{RenderScene, Shading};
use crate::rendering::texture::TextureAsset;

/// Longest step, in seconds, that [`AssetScene::step`] simulates at once; longer stalls are clamped.
const MAXIMUM_STEP_SECONDS: f64 = 0.1;

/// Particle effect file extension.
const EFFECT_EXTENSIONS: [&str; 1] = [".joyfx"];

/// Name of the synthetic level used to resolve linked objects.
const PRESENTATION_NAME: &str = "Presentation";

/// A failure while preparing or committing presentation. The running presentation is unchanged.
#[derive(Debug, thiserror::Error)]
pub enum AssetSceneError {
    /// The scene was destroyed.
    #[error("AssetScene is destroyed.")]
    Destroyed,
    /// Linked `.joyobject` templates failed to read, parse, or resolve.
    #[error(transparent)]
    Objects(#[from] ObjectError),
    /// A `render`, `light`, or `effect` component was invalid.
    #[error("Entity {entity}: {source}")]
    Component {
        /// The entity's identifier.
        entity: String,
        /// What was wrong.
        source: ComponentError,
    },
    /// The caller's reader failed for an asset path.
    #[error("Could not read {path}: {message}")]
    Read {
        /// The validated asset path.
        path: String,
        /// The reader's error message.
        message: String,
    },
    /// A FORM or FORM LAB mesh failed to compile.
    #[error("{path}: {source}")]
    Mesh {
        /// The mesh asset path.
        path: String,
        /// The compilation failure.
        source: RenderError,
    },
    /// A `.joyfx` document was invalid.
    #[error("{path}: {source}")]
    Effect {
        /// The effect asset path.
        path: String,
        /// The validation or script failure.
        source: ParticleAssetError,
    },
    /// An effect's embedded texture failed to decode.
    #[error("{path}: {source}")]
    Texture {
        /// The effect asset path.
        path: String,
        /// The decoding failure.
        source: ParticleTextureError,
    },
    /// An effect component's settings were invalid (`parameters` not a record).
    #[error("Entity {entity}: asset parameters must be a record.")]
    InvalidParameters {
        /// The entity's identifier.
        entity: String,
    },
    /// An effect could not start: an invalid seed or parameter, or a failing spawn or first update.
    #[error("Entity {entity}: {source}")]
    Playback {
        /// The entity's identifier.
        entity: String,
        /// The effect failure.
        source: ParticleEffectError,
    },
    /// Presenting the entities failed.
    #[error(transparent)]
    Level(#[from] LevelSceneError),
}

/// A running per-entity effect.
pub struct EffectInstance {
    effect: ParticleEffect,
    key: String,
    looping: bool,
}

impl EffectInstance {
    /// The simulation.
    pub fn effect(&self) -> &ParticleEffect {
        &self.effect
    }

    /// Whether the effect restarts once it has no live particles or pending emission.
    pub fn is_looping(&self) -> bool {
        self.looping
    }
}

/// A prepared replacement presentation; pass it to [`AssetScene::commit`]. Dropping it discards its effects.
pub struct AssetSceneStage {
    generation: u64,
    entities: Vec<EntityDefinition>,
    meshes: HashMap<String, MeshAsset>,
    effects: Vec<(String, StagedEffect)>,
    effect_cache: HashMap<String, CompiledParticleEffect>,
    textures: HashMap<Arc<str>, TextureAsset>,
}

impl AssetSceneStage {
    /// The resolved entities (linked objects expanded) this stage presents.
    pub fn entities(&self) -> &[EntityDefinition] {
        &self.entities
    }
}

/// How a staged entity effect is obtained at commit.
enum StagedEffect {
    /// The running effect with an identical key keeps playing.
    Reuse,
    /// A new effect, already stepped once.
    Created(Box<EffectInstance>),
}

/// Owns authored asset presentation, compiled caches, and per-entity effect simulation.
///
/// Mesh instances live in the [`RenderScene`] passed to [`commit`](Self::commit), [`sync`](Self::sync), and
/// [`update`](Self::update); always pass the same scene, and call [`destroy`](Self::destroy) to remove them.
pub struct AssetScene {
    // Displayed presentation.
    level: LevelScene,
    entities: Vec<EntityDefinition>,
    meshes: HashMap<String, MeshAsset>,
    effects: IndexMap<String, EffectInstance>,
    textures: ParticleTextureSet,

    // Compiled caches, pruned to the displayed presentation at each commit.
    library: FormAssetLibrary,
    effect_cache: HashMap<String, CompiledParticleEffect>,

    // Staging and lifecycle.
    generation: u64,
    destroyed: bool,
    error: String,
}

impl Default for AssetScene {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetScene {
    /// An empty presentation.
    pub fn new() -> Self {
        Self {
            level: LevelScene::new(),
            entities: Vec::new(),
            meshes: HashMap::new(),
            effects: IndexMap::new(),
            textures: ParticleTextureSet::new(),
            library: FormAssetLibrary::new(),
            effect_cache: HashMap::new(),
            generation: 0,
            destroyed: false,
            error: String::new(),
        }
    }

    /// Resolve, read, compile, and start everything `entities` need, without changing the displayed presentation.
    ///
    /// `read` receives validated `assets/…` paths (`.joyobject`, `.form`, `.formlab`, `.joyfx`) and returns their text;
    /// each path is read at most once per preparation, and the reader must enforce its own project boundary. Changed
    /// effects start with their component seed (default 7777) and parameters and advance one fixed step; effects whose
    /// asset text and settings are unchanged keep running. Starting a preparation supersedes earlier uncommitted ones.
    pub fn prepare<E: Display>(
        &mut self,
        entities: &[EntityDefinition],
        mut read: impl FnMut(&str) -> Result<String, E>,
    ) -> Result<AssetSceneStage, AssetSceneError> {
        if self.destroyed {
            return Err(AssetSceneError::Destroyed);
        }
        self.generation += 1;
        let generation = self.generation;
        let mut reads: HashMap<String, String> = HashMap::new();
        let mut cached_read = |path: &str| -> Result<String, String> {
            if let Some(text) = reads.get(path) {
                return Ok(text.clone());
            }
            let text = read(path).map_err(|error| error.to_string())?;
            reads.insert(path.to_owned(), text.clone());
            Ok(text)
        };

        let result = self.stage(generation, entities, &mut cached_read);
        if result.is_err() {
            // Compiled meshes the failed preparation added are not displayed; drop them like TypeScript's staging cache.
            let meshes = &self.meshes;
            self.library.retain(|asset| meshes.values().any(|mesh| mesh.ptr_eq(asset)));
        }
        result
    }

    /// Replace the displayed presentation with `stage` and present it in `scene` with `selected` highlighted.
    ///
    /// Returns `Ok(false)`, discarding the stage, when a newer preparation or [`cancel_pending`](Self::cancel_pending)
    /// superseded it or the scene was destroyed. On error nothing changes.
    pub fn commit(
        &mut self,
        scene: &mut RenderScene,
        stage: AssetSceneStage,
        selected: Option<&str>,
    ) -> Result<bool, AssetSceneError> {
        if self.destroyed || stage.generation != self.generation {
            return Ok(false);
        }
        let AssetSceneStage { entities, meshes, effects, effect_cache, textures, .. } = stage;

        // LevelScene validates every record before changing the scene, so presenting first keeps commit atomic.
        self.level.sync_with(scene, &entities, selected, |id, _| Ok(meshes.get(id).cloned()))?;

        let mut previous = std::mem::take(&mut self.effects);
        let mut running = IndexMap::with_capacity(effects.len());
        for (id, staged) in effects {
            match staged {
                StagedEffect::Reuse => {
                    if let Some(instance) = previous.shift_remove(&id) {
                        running.insert(id, instance);
                    }
                }
                StagedEffect::Created(instance) => {
                    running.insert(id, *instance);
                }
            }
        }
        for (_, mut instance) in previous {
            instance.effect.destroy();
        }

        self.effects = running;
        self.meshes = meshes;
        self.effect_cache = effect_cache;
        self.textures = ParticleTextureSet::from_textures(textures);
        let displayed = &self.meshes;
        self.library.retain(|asset| displayed.values().any(|mesh| mesh.ptr_eq(asset)));
        self.entities = entities;
        self.error.clear();
        Ok(true)
    }

    /// [`prepare`](Self::prepare) then [`commit`](Self::commit): returns `Ok(false)` only for a superseded load.
    pub fn sync<E: Display>(
        &mut self,
        scene: &mut RenderScene,
        entities: &[EntityDefinition],
        selected: Option<&str>,
        read: impl FnMut(&str) -> Result<String, E>,
    ) -> Result<bool, AssetSceneError> {
        let stage = self.prepare(entities, read)?;
        self.commit(scene, stage, selected)
    }

    /// Invalidate prepared stages while keeping the displayed presentation; call when a newer owner transaction starts
    /// before its own asset reads are ready.
    pub fn cancel_pending(&mut self) {
        self.generation += 1;
    }

    /// Copy current transforms and selection without reading assets or restarting effects.
    ///
    /// Entities without a prepared mesh draw their fallback primitive. Does nothing after destruction.
    pub fn update(
        &mut self,
        scene: &mut RenderScene,
        entities: &[EntityDefinition],
        selected: Option<&str>,
    ) -> Result<(), AssetSceneError> {
        if self.destroyed {
            return Ok(());
        }
        let meshes = &self.meshes;
        self.level.sync_with(scene, entities, selected, |id, _| Ok(meshes.get(id).cloned()))?;
        self.entities = entities.to_vec();
        Ok(())
    }

    /// Advance effects by `seconds` (clamped to 0.1), restarting finished looping effects.
    ///
    /// Script failures stop only the failing effect and are reported through [`error`](Self::error); invalid or
    /// negative times are ignored.
    pub fn step(&mut self, seconds: f64) {
        if self.destroyed || !seconds.is_finite() || seconds < 0.0 {
            return;
        }
        for instance in self.effects.values_mut() {
            let effect = &mut instance.effect;
            if effect.error().is_some() {
                continue;
            }
            let mut advance = || -> Result<(), ParticleEffectError> {
                if instance.looping && !effect.is_alive() {
                    effect.reset()?;
                }
                effect.update(seconds.min(MAXIMUM_STEP_SECONDS))
            };
            if let Err(error) = advance() {
                self.error = error.to_string();
            }
        }
    }

    /// Camera-facing colored particles of every entity's effect, in world space, attached to entity transforms.
    ///
    /// The result is unsorted; pass it as `FrameInputs::transparent_triangles`. Textured and flipbook emitters are
    /// omitted here; draw them with [`particle_batches`](Self::particle_batches).
    pub fn particles(&self, camera: &ParticleCameraBasis) -> Vec<ColorVertex> {
        let mut vertices = Vec::new();
        for (entity, instance) in self.attached_effects() {
            let basis = local_basis(camera, entity);
            let Ok(triangles) = collect_particle_triangles(&instance.effect, &basis, None, false) else {
                continue;
            };
            for triangle in triangles {
                for corner in triangle.corners {
                    let world = to_world(entity, corner.position);
                    vertices.push(ColorVertex {
                        position: world.map(|value| value as f32),
                        color: corner.color.map(|value| value as f32),
                    });
                }
            }
        }
        vertices
    }

    /// Every entity's particles, colored and textured, as world-space material runs sorted back to front across
    /// entities. Draw them with [`transparent_draws`](Self::transparent_draws).
    pub fn particle_batches(&self, camera: &ParticleCameraBasis) -> Result<Vec<ParticleBatch>, ParticleRenderError> {
        let mut triangles: Vec<ParticleTriangle> = Vec::new();
        for (entity, instance) in self.attached_effects() {
            let basis = local_basis(camera, entity);
            for mut triangle in collect_particle_triangles(&instance.effect, &basis, None, true)? {
                for corner in &mut triangle.corners {
                    corner.position = to_world(entity, corner.position);
                }
                triangles.push(triangle);
            }
        }
        let depths = triangles.iter().map(|triangle| triangle.depth(&camera.backward)).collect();
        Ok(batch_particle_triangles(triangles, depths))
    }

    /// Borrow renderer draws for batches from [`particle_batches`](Self::particle_batches), using this scene's decoded
    /// effect textures.
    pub fn transparent_draws<'a>(
        &'a self,
        batches: &'a [ParticleBatch],
    ) -> Result<Vec<TransparentDraw<'a>>, ParticleTextureError> {
        self.textures.transparent_draws(batches)
    }

    /// The strongest particle lights of every effect in world space (positions follow entity transforms; ranges stay
    /// in effect units), at most `max_lights` (clamped to 32).
    pub fn particle_lights(&self, max_lights: usize) -> ParticleLightSelection {
        let limit = max_lights.min(MAX_POINT_LIGHTS);
        let mut lights = Vec::new();
        let mut total = 0;
        for (entity, instance) in self.attached_effects() {
            // Each effect's own top lights are enough to find the global top lights.
            let selection = collect_particle_lights(&instance.effect, MAX_POINT_LIGHTS);
            total += selection.lights.len() + selection.omitted;
            for mut light in selection.lights {
                let [x, y, z] = to_world(entity, [light.x, light.y, light.z]);
                (light.x, light.y, light.z) = (x, y, z);
                lights.push(light);
            }
        }
        rank_particle_lights(&mut lights);
        lights.truncate(limit);
        let omitted = total - lights.len();
        ParticleLightSelection { lights, omitted }
    }

    /// Level `light` components followed by the strongest particle lights, at most 32 in total.
    pub fn lights(&self) -> Vec<PointLight> {
        let mut lights: Vec<PointLight> = self.level.lights().iter().take(MAX_POINT_LIGHTS).copied().collect();
        let remaining = MAX_POINT_LIGHTS - lights.len();
        lights.extend(self.particle_lights(remaining).point_lights());
        lights
    }

    /// Remove every instance from `scene`, stop every effect, and invalidate prepared stages. Idempotent; later
    /// preparations fail with [`AssetSceneError::Destroyed`].
    pub fn destroy(&mut self, scene: &mut RenderScene) {
        if self.destroyed {
            return;
        }
        self.destroyed = true;
        self.generation += 1;
        for instance in self.effects.values_mut() {
            instance.effect.destroy();
        }
        self.effects.clear();
        self.meshes.clear();
        self.library.clear();
        self.effect_cache.clear();
        self.textures.clear();
        self.level.clear(scene);
    }

    /// The displayed entities (linked objects resolved).
    pub fn entities(&self) -> &[EntityDefinition] {
        &self.entities
    }

    /// The compiled mesh presenting `entity_id`'s `render` asset.
    pub fn mesh(&self, entity_id: &str) -> Option<&MeshAsset> {
        self.meshes.get(entity_id)
    }

    /// The running effect of `entity_id`.
    pub fn effect(&self, entity_id: &str) -> Option<&EffectInstance> {
        self.effects.get(entity_id)
    }

    /// Running effects by entity id, in entity order.
    pub fn effects(&self) -> impl Iterator<Item = (&str, &EffectInstance)> {
        self.effects.iter().map(|(id, instance)| (id.as_str(), instance))
    }

    /// The level presentation (instances and light components).
    pub fn level(&self) -> &LevelScene {
        &self.level
    }

    /// The most recent effect failure reported by [`step`](Self::step); empty when none since the last commit.
    pub fn error(&self) -> &str {
        &self.error
    }

    /// Whether [`destroy`](Self::destroy) was called.
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Staging internals
    // -----------------------------------------------------------------------------------------------------------------

    /// Build a stage for `entities`; touches only the mesh cache (pruned by the caller on failure).
    fn stage(
        &mut self,
        generation: u64,
        entities: &[EntityDefinition],
        read: &mut impl FnMut(&str) -> Result<String, String>,
    ) -> Result<AssetSceneStage, AssetSceneError> {
        let mut level = LevelData::new(PRESENTATION_NAME);
        level.entities = entities.to_vec();
        let resolved = resolve_level_objects(&level, &mut *read)?.entities;
        let mut read_asset =
            |path: &str| read(path).map_err(|message| AssetSceneError::Read { path: path.to_owned(), message });

        let mut meshes = HashMap::new();
        let mut effects = Vec::new();
        let mut effect_cache: HashMap<String, CompiledParticleEffect> = HashMap::new();
        let mut textures: HashMap<Arc<str>, TextureAsset> = HashMap::new();
        for entity in &resolved {
            let component_error = |source| AssetSceneError::Component { entity: entity.id.clone(), source };

            // Validate presentation records now so commit cannot fail on them.
            if let Some(record) = entity.components.get(component_type::LIGHT) {
                LightComponent::read(record).map_err(component_error)?;
            }
            if let Some(record) = entity.components.get(component_type::RENDER)
                && let RenderSource::Mesh { asset, parameters } =
                    RenderComponent::read(record).map_err(component_error)?.source
            {
                let text = read_asset(&asset)?;
                let mesh = self
                    .library
                    .resolve(&asset, &text, &parameters, Shading::Lit)
                    .map_err(|source| AssetSceneError::Mesh { path: asset.clone(), source })?;
                meshes.insert(entity.id.clone(), mesh);
            }

            let Some(settings) = entity.components.get(component_type::EFFECT) else {
                continue;
            };
            let enabled = settings.get("enabled") != Some(&Value::Bool(false));
            let path = match settings.get("asset") {
                Some(value) if is_truthy(value) && enabled => match value {
                    Value::String(path) => asset_path(path, &EFFECT_EXTENSIONS)
                        .map_err(|error| component_error(ComponentError::AssetPath(error)))?
                        .to_owned(),
                    _ => return Err(component_error(ComponentError::AssetPath(AssetPathError::OutsideAssets))),
                },
                _ => continue,
            };
            let text = read_asset(&path)?;
            let definition = match effect_cache.get(&text).or_else(|| self.effect_cache.get(&text)) {
                Some(definition) => definition.clone(),
                None => parse_particle_effect(&text)
                    .map_err(|source| AssetSceneError::Effect { path: path.clone(), source })?,
            };
            effect_cache.insert(text.clone(), definition.clone());
            for source in textured_sources(&definition) {
                if textures.contains_key(&source) {
                    continue;
                }
                let texture = match self.textures.get(&source) {
                    Ok(existing) => existing.clone(),
                    Err(_) => ParticleTextureSet::decode(&source)
                        .map_err(|error| AssetSceneError::Texture { path: path.clone(), source: error })?,
                };
                textures.insert(source, texture);
            }

            let key = serde_json::json!([text, settings]).to_string();
            if self.effects.get(&entity.id).is_some_and(|previous| previous.key == key) {
                effects.push((entity.id.clone(), StagedEffect::Reuse));
                continue;
            }
            let instance = start_effect(&entity.id, &definition, settings, key)?;
            effects.push((entity.id.clone(), StagedEffect::Created(Box::new(instance))));
        }
        Ok(AssetSceneStage { generation, entities: resolved, meshes, effects, effect_cache, textures })
    }

    /// Entities that have a running effect, in entity order.
    fn attached_effects(&self) -> impl Iterator<Item = (&EntityDefinition, &EffectInstance)> {
        self.entities.iter().filter_map(|entity| self.effects.get(&entity.id).map(|instance| (entity, instance)))
    }
}

/// Create an effect from component settings and advance it one fixed step, as TypeScript does on sync.
fn start_effect(
    entity: &str,
    definition: &CompiledParticleEffect,
    settings: &serde_json::Map<String, Value>,
    key: String,
) -> Result<EffectInstance, AssetSceneError> {
    let playback = |source| AssetSceneError::Playback { entity: entity.to_owned(), source };
    let seed = match settings.get("seed") {
        None => EFFECT_COMPONENT_DEFAULTS.seed,
        Some(value) => particle_seed(javascript_number(value)).map_err(playback)?,
    };
    let parameters = match settings.get("parameters") {
        None => Vec::new(),
        Some(Value::Object(record)) => {
            let mut parameters = Vec::with_capacity(record.len());
            for (name, value) in record {
                let Some(value) = ParticleParameterValue::from_json(value) else {
                    return Err(playback(ParticleEffectError::InvalidParameter(name.clone())));
                };
                parameters.push((name.clone(), value));
            }
            parameters
        }
        Some(_) => return Err(AssetSceneError::InvalidParameters { entity: entity.to_owned() }),
    };
    let options = ParticleEffectOptions { seed: i64::from(seed), parameters, ..Default::default() };
    let mut effect = ParticleEffect::new(definition, options).map_err(playback)?;
    effect.step().map_err(playback)?;
    let looping = settings.get("loop") == Some(&Value::Bool(true));
    Ok(EffectInstance { effect, key, looping })
}

/// JavaScript truthiness of an authored JSON value.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0 && !number.is_nan()),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `Number(value)` for the JSON values a seed may hold; arrays and objects become NaN.
fn javascript_number(value: &Value) -> f64 {
    match value {
        Value::Null => 0.0,
        Value::Bool(flag) => f64::from(u8::from(*flag)),
        Value::Number(number) => number.as_f64().unwrap_or(f64::NAN),
        Value::String(text) => {
            let trimmed = text.trim();
            match trimmed {
                "" => 0.0,
                "Infinity" | "+Infinity" => f64::INFINITY,
                "-Infinity" => f64::NEG_INFINITY,
                _ if trimmed.chars().all(|character| character.is_ascii_digit() || "+-.eE".contains(character)) => {
                    trimmed.parse().unwrap_or(f64::NAN)
                }
                _ => f64::NAN,
            }
        }
        Value::Array(_) | Value::Object(_) => f64::NAN,
    }
}

/// The camera basis expressed in an entity's rotated frame, so attached particles stay camera-facing.
fn local_basis(camera: &ParticleCameraBasis, entity: &EntityDefinition) -> ParticleCameraBasis {
    let angles = entity.transform.rotation.to_array().map(f64::from);
    ParticleCameraBasis {
        right: rotate(camera.right, angles, true),
        up: rotate(camera.up, angles, true),
        backward: rotate(camera.backward, angles, true),
    }
}

/// Transform an effect-space point by the entity's scale, rotation, and position.
fn to_world(entity: &EntityDefinition, point: [f64; 3]) -> [f64; 3] {
    let transform = &entity.transform;
    let scale = transform.scale.to_array().map(f64::from);
    let angles = transform.rotation.to_array().map(f64::from);
    let position = transform.position.to_array().map(f64::from);
    let rotated = rotate([point[0] * scale[0], point[1] * scale[1], point[2] * scale[2]], angles, false);
    [rotated[0] + position[0], rotated[1] + position[1], rotated[2] + position[2]]
}

/// Rotate a direction with the engine's XYZ Euler convention (X, then Y, then Z), or its inverse.
fn rotate(point: [f64; 3], angles: [f64; 3], inverse: bool) -> [f64; 3] {
    let [mut x, mut y, mut z] = point;
    let axes = if inverse { [2, 1, 0] } else { [0, 1, 2] };
    for axis in axes {
        let angle = angles[axis] * if inverse { -1.0 } else { 1.0 };
        let (cosine, sine) = (libm::cos(angle), libm::sin(angle));
        match axis {
            0 => (y, z) = ((y * cosine) - (z * sine), (y * sine) + (z * cosine)),
            1 => (x, z) = ((x * cosine) + (z * sine), (-x * sine) + (z * cosine)),
            _ => (x, y) = ((x * cosine) - (y * sine), (x * sine) + (y * cosine)),
        }
    }
    [x, y, z]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_inverts() {
        let angles = [0.3, -1.1, 2.0];
        let point = [1.0, 2.0, 3.0];
        let back = rotate(rotate(point, angles, false), angles, true);
        for axis in 0..3 {
            assert!((back[axis] - point[axis]).abs() < 1e-12);
        }
    }

    #[test]
    fn seeds_coerce_like_javascript_numbers() {
        assert_eq!(javascript_number(&Value::from("12")), 12.0);
        assert_eq!(javascript_number(&Value::Null), 0.0);
        assert!(javascript_number(&Value::from("seven")).is_nan());
    }
}
