// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! GPU-free descriptions of what the renderer draws: [`mesh`]es, [`material`]s, [`light`]s, and [`document`]s that
//! pair a mesh with its slots' materials.
//!
//! These are plain records. Content producers (FORM, glTF import, destructible structures, the environment
//! simulation, and games) write them, and the renderer reads them, so this crate sits in the foundation layer below
//! both. It never names wgpu, GPU resources, or a particular renderer: anything that allocates or binds GPU memory
//! belongs to `joy-renderer`, which converts these records into its own assets.
//!
//! Records follow the engine's coordinate conventions (Y-up, right-handed, meters) and keep colors as linear light
//! ([`joy_core::LinearColor`]).

pub mod document;
pub mod light;
pub mod material;
pub mod mesh;

pub use document::{
    MESH_DOCUMENT_VERSION,
    MeshDocument,
    MeshDocumentError,
};
pub use light::{
    DirectionalLight,
    PointLight,
    SUN_ANGULAR_RADIUS,
    Skylight,
};
pub use material::{
    AlphaMode,
    Coat,
    CustomShading,
    Fuzz,
    Haze,
    LitSurface,
    Material,
    MaterialError,
    ShaderValue,
    Shading,
    ShadingModel,
    Surface,
    SurfaceLayer,
    SurfaceMaps,
    TextureSource,
    ThinFilm,
    UnlitSurface,
};
pub use mesh::{
    MaterialSlot,
    Mesh,
    MeshAttribute,
    MeshError,
};
