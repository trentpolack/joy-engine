// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Authored asset presentation for levels, the editor, and previews: FORM meshes plus per-entity particle effects.
//!
//! This owner belongs to the rendering layer because it presents meshes and effects. Entities' `render` components
//! resolve FORM and
//! FORM EDITOR meshes through a [`FormAssetLibrary`] and draw through [`LevelScene`]; `effect` components play `.joyfx`
//! effects attached to the entity transform, with the component's `seed`, `loop`, `enabled`, and `parameters`.
//!
//! Synchronization is staged: [`AssetScene::prepare`] reads, compiles, decodes, and starts every
//! resource without touching the displayed presentation, and [`AssetScene::commit`] swaps it in only if no newer
//! preparation (or [`cancel_pending`](AssetScene::cancel_pending)) superseded it. A failed preparation leaves the running
//! presentation, its effects, and its caches unchanged. Asset text always comes from the caller's `read` function (a
//! file system, a bundle, or text fetched by a browser), never from this module, so it works the same in browsers.
//!
//! FORM assets draw with [`LEVEL_FORM_SHADING`] (the layered model FORM EDITOR previews with), so a placed asset keeps
//! its palette, material factors, and embedded maps instead of collapsing to vertex colors.
//!
//! Effects draw through the renderer's one particle path: [`AssetScene::add_particles`] adds every entity's effect,
//! textured and flipbook emitters included, to a caller-owned `ParticleFrame` at its entity transform, and particle
//! lights are available in world space.

use std::collections::{
    HashMap,
    HashSet,
};
use std::fmt::Display;
use std::sync::Arc;

use crate::components::{
    ComponentError,
    EFFECT_COMPONENT_DEFAULTS,
    LightComponent,
    RenderComponent,
    RenderSource,
};
use indexmap::IndexMap;
use joy_core::Transform;
use joy_core::math::Mat4;
use joy_form::CookedForms;
use joy_fx::{
    CompiledParticleEffect,
    ParticleAssetError,
    ParticleEffect,
    ParticleEffectError,
    ParticleEffectOptions,
    ParticleParameterValue,
    ParticleTextureError,
    parse_particle_effect,
    particle_seed,
};
use joy_gameplay::{
    AssetPathError,
    EntityDefinition,
    LevelData,
    ObjectError,
    asset_path,
    component_type,
    resolve_level_objects,
};
use joy_renderer::particles::{
    ParticleFrame,
    ParticleLightSelection,
    ParticleRenderError,
    ParticleTextureSet,
    collect_particle_lights,
    rank_particle_lights,
    textured_sources,
};
use joy_renderer::{
    MAX_POINT_LIGHTS,
    MeshAsset,
    PointLight,
    RenderScene,
    ShadingModel,
    TextureAsset,
};
use serde_json::Value;

use crate::form_assets::{
    FormAssetLibrary,
    FormMeshError,
};
use crate::form_placements::{
    FormPlacement,
    PLACEMENT_ID_SEPARATOR,
    PlacementLimitError,
    placement_entities,
};
use crate::level_scene::{
    LevelEntity,
    LevelScene,
    LevelSceneError,
};

/// Longest step, in seconds, that [`AssetScene::step`] simulates at once; longer stalls are clamped.
const MAXIMUM_STEP_SECONDS: f64 = 0.1;

/// Particle effect file extension.
const EFFECT_EXTENSIONS: [&str; 1] = [".joyfx"];

/// Shading model for FORM assets placed in levels: the layered model FORM EDITOR previews with, so authored palettes,
/// material factors, and embedded maps look the same in the editor and in play.
pub const LEVEL_FORM_SHADING: ShadingModel<'static> = ShadingModel::Layered;

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
    /// A FORM or FORM EDITOR mesh failed to compile.
    #[error("{path}: {source}")]
    Mesh {
        /// The mesh asset path.
        path: String,
        /// The compilation failure.
        source: FormMeshError,
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
    /// A FORM asset named more object placements than one asset may spawn.
    #[error(transparent)]
    Placements(#[from] PlacementLimitError),
    /// A spawned placement entity's id is already used by an authored entity.
    #[error("Entity {entity}: a FORM placement entity already has this id.")]
    PlacementId {
        /// The conflicting id.
        entity: String,
    },
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

/// A FORM placement expanded into an entity of its own: the placement (to follow its parent) and the resolved entity.
#[derive(Clone, Debug)]
struct SpawnedEntity {
    placement: FormPlacement,
    definition: EntityDefinition,
}

/// Resources gathered while staging; becomes the displayed presentation at commit.
#[derive(Default)]
struct Staging {
    meshes: HashMap<String, MeshAsset>,
    placements: HashMap<String, Arc<[FormPlacement]>>,
    effects: Vec<(String, StagedEffect)>,
    effect_cache: HashMap<String, CompiledParticleEffect>,
    textures: HashMap<Arc<str>, TextureAsset>,
}

/// A prepared replacement presentation; pass it to [`AssetScene::commit`]. Dropping it discards its effects.
pub struct AssetSceneStage {
    generation: u64,
    entities: Vec<EntityDefinition>,
    spawned: IndexMap<String, Vec<SpawnedEntity>>,
    staging: Staging,
}

impl AssetSceneStage {
    /// The resolved entities this stage presents: linked objects expanded, followed by the entities spawned for FORM
    /// object placements (see [`placement_entities`](crate::placement_entities)). Games hand this list to a
    /// `LevelRuntime` so spawned entities simulate like authored ones.
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
    placements: HashMap<String, Arc<[FormPlacement]>>,
    spawned: IndexMap<String, Vec<SpawnedEntity>>,
    effects: IndexMap<String, EffectInstance>,
    textures: ParticleTextureSet,
    // Presentation records of running entities no stage prepared (spawned at runtime), keyed by entity id.
    adopted: HashMap<String, EntityDefinition>,

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
            placements: HashMap::new(),
            spawned: IndexMap::new(),
            effects: IndexMap::new(),
            textures: ParticleTextureSet::new(),
            adopted: HashMap::new(),
            library: FormAssetLibrary::new(),
            effect_cache: HashMap::new(),
            generation: 0,
            destroyed: false,
            error: String::new(),
        }
    }

    /// Load FORM geometry from `cooked` (a game's `joy_engine::cooked_forms!`) whenever a level asset's evaluation
    /// inputs were cooked at build time; everything else, including hot-reloaded edits, evaluates live.
    pub fn set_cooked_forms(&mut self, cooked: CookedForms) {
        self.library.set_cooked_forms(cooked);
    }

    /// Resolve, read, compile, and start everything `entities` need, without changing the displayed presentation.
    ///
    /// `read` receives validated `assets/…` paths (`.joyobject`, `.form`, `.forminst`, `.joyfx`, and the scripts and
    /// modules FORM assets reference) and returns their text;
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
            // Compiled meshes the failed preparation added are not displayed; drop them so the library holds only
            // displayed meshes.
            let meshes = &self.meshes;
            self.library.retain(|asset| meshes.values().any(|mesh| mesh.ptr_eq(asset)));
        }
        result
    }

    /// [`prepare`](Self::prepare) a whole level and return it ready to run: linked objects resolved and FORM object
    /// placements expanded into entities, in the order [`AssetSceneStage::entities`] lists them.
    ///
    /// This is the one call a game needs to build its world from an authored level:
    ///
    /// ```ignore
    /// let (runtime_level, stage) = assets.prepare_level(&level, |path| game_assets.read(path))?;
    /// let runtime = LevelRuntime::new(&runtime_level, registry)?;
    /// assets.commit(&mut scene, stage, None)?;
    /// // Every frame: assets.follow(&mut scene, runtime.world().presented_entities(), None)?;
    /// ```
    pub fn prepare_level<E: Display>(
        &mut self,
        level: &LevelData,
        read: impl FnMut(&str) -> Result<String, E>,
    ) -> Result<(LevelData, AssetSceneStage), AssetSceneError> {
        let stage = self.prepare(&level.entities, read)?;
        let mut runtime_level = level.clone();
        runtime_level.entities = stage.entities.clone();
        Ok((runtime_level, stage))
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
        let AssetSceneStage {
            entities,
            spawned,
            staging:
                Staging {
                    meshes,
                    placements,
                    effects,
                    effect_cache,
                    textures,
                },
            ..
        } = stage;

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
        self.placements = placements;
        self.spawned = spawned;
        self.effect_cache = effect_cache;
        self.textures = ParticleTextureSet::new();
        self.textures.insert_textures(textures);
        self.adopted.clear();
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

    /// Copy current transforms and selection of authored `entities` without reading assets or restarting effects.
    ///
    /// Entities spawned for FORM object placements follow their parent's current transform (unless `entities` already
    /// lists them). Entities without a prepared mesh draw their fallback primitive. Does nothing after destruction.
    pub fn update(
        &mut self,
        scene: &mut RenderScene,
        entities: &[EntityDefinition],
        selected: Option<&str>,
    ) -> Result<(), AssetSceneError> {
        if self.destroyed {
            return Ok(());
        }
        let presented = self.with_spawned(entities);
        let meshes = &self.meshes;
        self.level.sync_with(scene, &presented, selected, |id, _| Ok(meshes.get(id).cloned()))?;
        self.entities = presented;
        Ok(())
    }

    /// Prepare presentation for running entities no stage prepared (spawned at runtime by the game or requested by
    /// behaviors): their FORM meshes, effects, and lights, read through `read` and cached like a stage's. Returns how
    /// many entities it adopted.
    ///
    /// Call it before [`follow`](Self::follow) on frames where entities may have spawned; entities already presented
    /// cost a lookup. An entity whose records fail to prepare (an unreadable asset, an invalid effect) is still adopted,
    /// drawing its fallback primitive without an effect, and the failure is reported through [`error`](Self::error), so
    /// it is not read again every frame. `follow` releases adopted presentation once its entity leaves, and a commit
    /// clears it so hot-reloaded assets are adopted afresh. Fails only after destruction.
    pub fn adopt<'a, E: LevelEntity + 'a, R: Display>(
        &mut self,
        entities: impl IntoIterator<Item = &'a E>,
        mut read: impl FnMut(&str) -> Result<String, R>,
    ) -> Result<usize, AssetSceneError> {
        if self.destroyed {
            return Err(AssetSceneError::Destroyed);
        }
        let prepared: HashSet<&str> = self.entities.iter().map(|entity| entity.id.as_str()).collect();
        let newcomers: Vec<EntityDefinition> = entities
            .into_iter()
            .filter(|entity| {
                let id = entity.entity_id();
                !prepared.contains(id) && !self.adopted.contains_key(id)
            })
            .map(|entity| {
                let mut definition = runtime_definition(entity);
                definition.transform = *entity.entity_transform();
                definition
            })
            .collect();
        if newcomers.is_empty() {
            return Ok(0);
        }

        let mut text_read = |path: &str| read(path).map_err(|error| error.to_string());
        let mut staging = Staging::default();
        for entity in &newcomers {
            if let Err(error) = self.stage_entity(entity, &mut text_read, &mut staging) {
                self.error = error.to_string();
            }
        }

        // Everything staged; adopt it without disturbing the running presentation.
        let Staging {
            meshes,
            effects,
            effect_cache,
            textures,
            ..
        } = staging;
        self.meshes.extend(meshes);
        for (id, staged) in effects {
            if let StagedEffect::Created(instance) = staged {
                self.effects.insert(id, *instance);
            }
        }
        self.effect_cache.extend(effect_cache);
        self.textures.insert_textures(textures);
        let adopted = newcomers.len();
        for entity in newcomers {
            self.adopted.insert(entity.id.clone(), entity);
        }
        Ok(adopted)
    }

    /// Present running entities (for example `World::presented_entities`) with the prepared assets, at their current
    /// transforms, without reading assets or restarting effects.
    ///
    /// Use this when a `LevelRuntime` built from [`AssetSceneStage::entities`] owns the simulation: spawned placement
    /// entities are runtime entities then, so they move (or disappear) with the simulation rather than their parent.
    /// Entities spawned at runtime present what [`adopt`](Self::adopt) prepared for them, or their `render` primitives
    /// and `light` components until adopted; adopted presentation is released once its entity is no longer listed.
    /// Does nothing after destruction.
    pub fn follow<'a, E: LevelEntity + 'a>(
        &mut self,
        scene: &mut RenderScene,
        entities: impl IntoIterator<Item = &'a E>,
        selected: Option<&str>,
    ) -> Result<(), AssetSceneError> {
        if self.destroyed {
            return Ok(());
        }
        let presented: Vec<EntityDefinition> = {
            let prepared: HashMap<&str, &EntityDefinition> =
                self.entities.iter().map(|entity| (entity.id.as_str(), entity)).collect();
            entities
                .into_iter()
                .map(|entity| {
                    let id = entity.entity_id();
                    let mut definition = match prepared.get(id).copied().or_else(|| self.adopted.get(id)) {
                        Some(definition) => definition.clone(),
                        None => runtime_definition(entity),
                    };
                    definition.transform = *entity.entity_transform();
                    definition
                })
                .collect()
        };
        let meshes = &self.meshes;
        self.level.sync_with(scene, &presented, selected, |id, _| Ok(meshes.get(id).cloned()))?;
        self.release_departed(&presented);
        self.entities = presented;
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

    /// Add every entity's effect to `frame`, placed by its entity transform (sizes scale with it; billboards stay
    /// camera-facing), with this scene's decoded textures. Clear `frame` first when it holds only this scene's
    /// particles; the renderer culls, sorts, and draws it (`FrameInputs::particles`).
    ///
    /// Fails, leaving the effects added so far, only if an effect's texture is missing, which a committed or adopted
    /// stage rules out.
    pub fn add_particles(&self, frame: &mut ParticleFrame) -> Result<(), ParticleRenderError> {
        for (entity, instance) in self.attached_effects() {
            frame.add_transformed_effect(&instance.effect, Some(&self.textures), &entity_matrix(entity))?;
        }
        Ok(())
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
            let placement = entity_matrix(entity);
            for mut selected in selection.lights {
                selected.light.position = placement.transform_point3(selected.light.position);
                lights.push(selected);
            }
        }
        rank_particle_lights(&mut lights);
        lights.truncate(limit);
        let omitted = total - lights.len();
        ParticleLightSelection {
            lights,
            omitted,
        }
    }

    /// Every level `light` component followed by the strongest particle lights (at most [`MAX_POINT_LIGHTS`] of them).
    ///
    /// Pass the whole list as `FrameInputs::lights`: the renderer keeps the lights that contribute most to the view, so
    /// a bright particle near the camera can outrank a distant level light.
    pub fn lights(&self) -> Vec<PointLight> {
        let mut lights = self.level.lights().to_vec();
        lights.extend(self.particle_lights(MAX_POINT_LIGHTS).point_lights());
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
        self.placements.clear();
        self.spawned.clear();
        self.adopted.clear();
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

    /// The placements `entity_id`'s FORM asset tagged, in the asset's local space (empty when it has none).
    pub fn placements(&self, entity_id: &str) -> &[FormPlacement] {
        self.placements.get(entity_id).map(|placements| &placements[..]).unwrap_or_default()
    }

    /// Every displayed entity's placements, by entity id, in the asset's local space.
    pub fn all_placements(&self) -> impl Iterator<Item = (&str, &[FormPlacement])> {
        self.placements.iter().map(|(id, placements)| (id.as_str(), &placements[..]))
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
    ///
    /// FORM placements that name `.joyobject` templates become entities of their own, one level deep: a spawned
    /// entity's own FORM asset is staged (and its placements reported), but its object placements are not expanded
    /// again, so content cannot recurse without bound.
    fn stage(
        &mut self,
        generation: u64,
        entities: &[EntityDefinition],
        read: &mut impl FnMut(&str) -> Result<String, String>,
    ) -> Result<AssetSceneStage, AssetSceneError> {
        let mut level = LevelData::new(PRESENTATION_NAME);
        level.entities = entities.to_vec();
        let mut resolved = resolve_level_objects(&level, &mut *read)?.entities;
        let mut staging = Staging::default();
        for entity in &resolved {
            self.stage_entity(entity, read, &mut staging)?;
        }

        // Expand object placements into resolved entities that follow their parents.
        let authored_ids: HashSet<String> = resolved.iter().map(|entity| entity.id.clone()).collect();
        let mut spawned: IndexMap<String, Vec<SpawnedEntity>> = IndexMap::new();
        let mut children = Vec::new();
        for parent in &resolved {
            let Some(placements) = staging.placements.get(&parent.id).cloned() else {
                continue;
            };
            let linked = placement_entities(parent, &placements)?;
            if linked.is_empty() {
                continue;
            }
            let mut linked_level = LevelData::new(PRESENTATION_NAME);
            linked_level.entities = linked;
            let mut records = Vec::new();
            for definition in resolve_level_objects(&linked_level, &mut *read)?.entities {
                if authored_ids.contains(&definition.id) {
                    return Err(AssetSceneError::PlacementId {
                        entity: definition.id,
                    });
                }
                let Some(placement) =
                    placement_index(&parent.id, &definition.id).and_then(|index| placements.get(index))
                else {
                    continue;
                };
                children.push(definition.clone());
                records.push(SpawnedEntity {
                    placement: placement.clone(),
                    definition,
                });
            }
            spawned.insert(parent.id.clone(), records);
        }
        for child in &children {
            self.stage_entity(child, read, &mut staging)?;
        }
        resolved.extend(children);

        Ok(AssetSceneStage {
            generation,
            entities: resolved,
            spawned,
            staging,
        })
    }

    /// Validate one resolved entity's presentation records and stage its FORM mesh, placements, and effect.
    fn stage_entity(
        &mut self,
        entity: &EntityDefinition,
        read: &mut impl FnMut(&str) -> Result<String, String>,
        staging: &mut Staging,
    ) -> Result<(), AssetSceneError> {
        let component_error = |source| AssetSceneError::Component {
            entity: entity.id.clone(),
            source,
        };
        let read_error = |path: &str, message: String| AssetSceneError::Read {
            path: path.to_owned(),
            message,
        };

        // Validate presentation records now so commit cannot fail on them.
        if let Some(record) = entity.components.get(component_type::LIGHT) {
            LightComponent::read(record).map_err(component_error)?;
        }
        if let Some(record) = entity.components.get(component_type::RENDER)
            && let RenderSource::Mesh {
                asset,
                parameters,
                variant,
            } = RenderComponent::read(record).map_err(component_error)?.source
        {
            let text = read(&asset).map_err(|message| read_error(&asset, message))?;
            let compiled = self
                .library
                .resolve_asset(&asset, &text, &mut *read, variant.as_deref(), &parameters, LEVEL_FORM_SHADING)
                .map_err(|source| AssetSceneError::Mesh {
                    path: asset.clone(),
                    source,
                })?;
            if !compiled.placements.is_empty() {
                staging.placements.insert(entity.id.clone(), compiled.placements);
            }
            staging.meshes.insert(entity.id.clone(), compiled.mesh);
        }

        let Some(settings) = entity.components.get(component_type::EFFECT) else {
            return Ok(());
        };
        let enabled = settings.get("enabled") != Some(&Value::Bool(false));
        // A disabled effect, or one with no asset assigned (absent, null, or empty), shows nothing.
        let path = match settings.get("asset") {
            _ if !enabled => return Ok(()),
            None | Some(Value::Null) => return Ok(()),
            Some(Value::String(path)) if path.is_empty() => return Ok(()),
            Some(Value::String(path)) => asset_path(path, &EFFECT_EXTENSIONS)
                .map_err(|error| component_error(ComponentError::AssetPath(error)))?
                .to_owned(),
            Some(_) => return Err(component_error(ComponentError::AssetPath(AssetPathError::OutsideAssets))),
        };
        let text = read(&path).map_err(|message| read_error(&path, message))?;
        let definition = match staging.effect_cache.get(&text).or_else(|| self.effect_cache.get(&text)) {
            Some(definition) => definition.clone(),
            None => parse_particle_effect(&text).map_err(|source| AssetSceneError::Effect {
                path: path.clone(),
                source,
            })?,
        };
        staging.effect_cache.insert(text.clone(), definition.clone());
        for (id, source) in textured_sources(&definition) {
            if staging.textures.contains_key(&source) {
                continue;
            }
            let texture = match self.textures.get(id) {
                Some(existing) => existing.clone(),
                None => ParticleTextureSet::decode(&source).map_err(|error| AssetSceneError::Texture {
                    path: path.clone(),
                    source: error,
                })?,
            };
            staging.textures.insert(source, texture);
        }

        let key = serde_json::json!([text, settings]).to_string();
        if self.effects.get(&entity.id).is_some_and(|previous| previous.key == key) {
            staging.effects.push((entity.id.clone(), StagedEffect::Reuse));
            return Ok(());
        }
        let instance = start_effect(&entity.id, &definition, settings, key)?;
        staging.effects.push((entity.id.clone(), StagedEffect::Created(Box::new(instance))));
        Ok(())
    }

    /// `entities` followed by the spawned placement entities of each, at their parent's current transform; spawned
    /// entities `entities` already lists are not repeated.
    fn with_spawned(&self, entities: &[EntityDefinition]) -> Vec<EntityDefinition> {
        let listed: HashSet<&str> = entities.iter().map(|entity| entity.id.as_str()).collect();
        let mut presented = entities.to_vec();
        for parent in entities {
            for child in self.spawned.get(&parent.id).into_iter().flatten() {
                if listed.contains(child.definition.id.as_str()) {
                    continue;
                }
                let mut definition = child.definition.clone();
                definition.transform = child.placement.world_transform(&parent.transform);
                presented.push(definition);
            }
        }
        presented
    }

    /// Release the meshes and effects of adopted entities that `presented` no longer lists.
    fn release_departed(&mut self, presented: &[EntityDefinition]) {
        if self.adopted.is_empty() {
            return;
        }
        let listed: HashSet<&str> = presented.iter().map(|entity| entity.id.as_str()).collect();
        let departed: Vec<String> = self.adopted.keys().filter(|id| !listed.contains(id.as_str())).cloned().collect();
        for id in departed {
            self.adopted.remove(&id);
            self.meshes.remove(&id);
            if let Some(mut instance) = self.effects.shift_remove(&id) {
                instance.effect.destroy();
            }
        }
    }

    /// Entities that have a running effect, in entity order.
    fn attached_effects(&self) -> impl Iterator<Item = (&EntityDefinition, &EffectInstance)> {
        self.entities.iter().filter_map(|entity| self.effects.get(&entity.id).map(|instance| (entity, instance)))
    }
}

/// Create an effect from component settings and advance it one fixed step, so it shows on its first frame.
fn start_effect(
    entity: &str,
    definition: &CompiledParticleEffect,
    settings: &serde_json::Map<String, Value>,
    key: String,
) -> Result<EffectInstance, AssetSceneError> {
    let playback = |source| AssetSceneError::Playback {
        entity: entity.to_owned(),
        source,
    };
    let seed = match settings.get("seed") {
        None => EFFECT_COMPONENT_DEFAULTS.seed,
        Some(value) => particle_seed(value.as_f64().unwrap_or(f64::NAN)).map_err(playback)?,
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
        Some(_) => {
            return Err(AssetSceneError::InvalidParameters {
                entity: entity.to_owned(),
            });
        }
    };
    let options = ParticleEffectOptions {
        seed: i64::from(seed),
        parameters,
        ..Default::default()
    };
    let mut effect = ParticleEffect::new(definition, options).map_err(playback)?;
    effect.step().map_err(playback)?;
    let looping = settings.get("loop") == Some(&Value::Bool(true));
    Ok(EffectInstance {
        effect,
        key,
        looping,
    })
}

/// The placement index encoded in a spawned entity id (`<parent id>/<index>`).
fn placement_index(parent_id: &str, entity_id: &str) -> Option<usize> {
    entity_id.strip_prefix(parent_id)?.strip_prefix(PLACEMENT_ID_SEPARATOR)?.parse().ok()
}

/// A presentation record for a running entity the stage did not prepare: its `render`, `light`, and `effect` components.
fn runtime_definition<E: LevelEntity>(entity: &E) -> EntityDefinition {
    let mut definition = EntityDefinition::new(entity.entity_id());
    for kind in [component_type::RENDER, component_type::LIGHT, component_type::EFFECT] {
        if let Some(record) = entity.entity_component(kind) {
            definition.components.insert(kind.to_owned(), record.clone());
        }
    }
    definition
}

/// The effect-to-world matrix of an entity: scale, then XYZ Euler rotation, then translation, as its meshes draw.
fn entity_matrix(entity: &EntityDefinition) -> Mat4 {
    let transform = &entity.transform;
    Transform::new(transform.position, transform.rotation, transform.scale).matrix()
}
