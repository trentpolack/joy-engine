// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scene presentation: the bridge between engine content and the renderer, so the renderer never depends on gameplay,
//! FORM, or particle documents.
//!
//! [`LevelScene`] presents authored levels and runtime entities, [`AssetScene`] adds FORM meshes and per-entity particle
//! effects, and the FORM adapters ([`compile_form_mesh`], [`form_mesh_asset`], [`FormAssetLibrary`]) are the only path
//! from FORM output to the screen, and [`environment_lighting`] lights frames from the environment simulation, so games, tools, and previews present the same content the same way. Everything here
//! draws through `joy-renderer`'s retained [`RenderScene`](joy_renderer::RenderScene); `joy_engine::rendering` combines
//! both crates for games and tools.

mod asset_scene;
mod environment;
mod form_assets;
mod level_scene;

pub use asset_scene::{
    AssetScene,
    AssetSceneError,
    AssetSceneStage,
    EffectInstance,
};
pub use environment::environment_lighting;
pub use form_assets::{
    FormAssetLibrary,
    FormMeshError,
    FormTextureCache,
    compile_form_mesh,
    form_mesh_asset,
    form_mesh_asset_with_textures,
    primitive_box,
    primitive_sphere,
};
pub use level_scene::{
    LevelEntity,
    LevelScene,
    LevelSceneError,
};
