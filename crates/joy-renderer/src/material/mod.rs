// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Materials on the renderer side: shared [`MaterialAsset`]s, the shading-model registry they draw with, and their GPU
//! copies.
//!
//! The records themselves ([`Material`](joy_render_types::Material), [`Shading`](joy_render_types::Shading), and
//! [`SurfaceLayer`](joy_render_types::SurfaceLayer)) live in `joy-render-types`, so content producers write materials
//! without depending on the renderer. A material asset validates one and resolves its textures; meshes bind assets to
//! their slots and instances may override them.

mod asset;
mod cache;
mod models;

pub use asset::MaterialAsset;
pub(crate) use cache::MaterialCache;
pub(crate) use models::{
    LIT_MODEL_ID,
    ModelId,
    ShadingModels,
    UNLIT_MODEL_ID,
};
pub use models::{
    ParameterDefinition,
    ShadingModelDefinition,
    TextureDefinition,
    TextureFallback,
};
