// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The level format's component vocabulary: the type names, preset ids, categories, asset kinds, and entity types the
//! engine interprets.
//!
//! Projects may author any component type; the engine preserves unknown records as data and interprets only the types
//! named here. Simulation components (`physics`) are read in this crate; presentation components (`render`,
//! `light`, `effect`, `postProcess`) are read by `joy-scene`, next to the scenes that present them. Keeping every name
//! here gives the format one vocabulary regardless of which crate interprets a type.

/// Authored component types interpreted by engine runtimes.
pub mod component_type {
    /// Primitive or FORM mesh presentation.
    pub const RENDER: &str = "render";
    /// World-space point light at the entity position.
    pub const LIGHT: &str = "light";
    /// Rigid body created by `LevelRuntime`.
    pub const PHYSICS: &str = "physics";
    /// Particle effect attached to the entity transform.
    pub const EFFECT: &str = "effect";
    /// The level's sky, sun and moon clock, and wind (the first entity carrying one wins).
    pub const ENVIRONMENT: &str = "environment";
    /// Post-processing settings applied inside the entity's volume, or everywhere when unbounded.
    pub const POST_PROCESS: &str = "postProcess";
    /// Game-script behavior: names the library script whose hooks drive the entity (see `ScriptSystem`).
    pub const SCRIPT: &str = "script";
}

/// Built-in component presets: named starting records offered when an author adds a component.
pub mod component_preset {
    /// A box primitive with a tint.
    pub const RENDER_PRIMITIVE: &str = "render.primitive";
    /// An unassigned FORM mesh.
    pub const RENDER_MESH: &str = "render.mesh";
    /// A warm point light.
    pub const LIGHT_POINT: &str = "light.point";
    /// A static box collider.
    pub const PHYSICS_BOX: &str = "physics.box";
    /// A static sphere collider.
    pub const PHYSICS_SPHERE: &str = "physics.sphere";
    /// An unassigned looping JOYFX effect.
    pub const EFFECT_PARTICLES: &str = "effect.particles";
    /// The engine's default sky, clock, and wind.
    pub const ENVIRONMENT_DEFAULT: &str = "environment.default";
    /// A low sun under scattered clouds with a steady breeze; the clock stands still.
    pub const ENVIRONMENT_DUSK: &str = "environment.dusk";
    /// An unbounded post-processing volume: the level's overall look.
    pub const POST_PROCESS_GLOBAL: &str = "postProcess.global";
    /// A box post-processing volume.
    pub const POST_PROCESS_BOX: &str = "postProcess.box";
    /// A sphere post-processing volume.
    pub const POST_PROCESS_SPHERE: &str = "postProcess.sphere";
}

/// Authoring groups for component schemas; values are display labels.
pub mod component_category {
    /// Meshes and primitives.
    pub const RENDERING: &str = "Rendering";
    /// Lights.
    pub const LIGHTING: &str = "Lighting";
    /// Rigid bodies and colliders.
    pub const PHYSICS: &str = "Physics";
    /// Particle effects.
    pub const EFFECTS: &str = "Effects";
    /// Project gameplay data.
    pub const GAMEPLAY: &str = "Gameplay";
}

/// Workspace asset kinds component asset fields may reference; each kind is also its file extension.
pub mod component_asset_kind {
    /// FORM scripts.
    pub const FORM: &str = "form";
    /// FORM instances: a FORM script with parameter overrides, materials, and variants.
    pub const FORM_INSTANCE: &str = "forminst";
    /// JOYFX particle effects.
    pub const PARTICLE_EFFECT: &str = "joyfx";
}

/// Built-in entity types registered by every [`EntityRegistry`](super::EntityRegistry).
pub mod entity_type {
    /// Plain entity; the default type of a definition.
    pub const ENTITY: &str = "entity";
    /// Controllable character.
    pub const CHARACTER: &str = "character";
    /// Character intended for player control (AI may also possess it).
    pub const PLAYER_CHARACTER: &str = "player-character";
}
