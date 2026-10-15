// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Rendering layer of Joy Engine: the definitive renderer for games, tools, and examples.
//!
//! The renderer draws a retained [`RenderScene`] from a [`Camera`] each frame on wgpu, which runs on WebGPU and WebGL 2
//! in browsers and on Vulkan, Metal, DirectX 12, or OpenGL natively. Shaders are authored once in WGSL.
//!
//! Frame structure, materials, and display processing follow the TypeScript engine so the same content looks the same:
//! - Lit (flat, double-sided vertex-color lighting), PBR (metallic-roughness), and Unlit material passes.
//! - The environment's primary light with a filtered directional shadow map (also on WebGL 2), ambient skylight, and up
//!   to 32 point lights.
//! - The procedural atmosphere sky driven by an environment frame.
//! - HDR scene targets, bloom, blurs, AgX/ACES/linear tone mapping, grading, vignette, antialiasing, and film grain.
//!
//! The renderer owns every GPU resource it creates; scenes, cameras, and frame inputs are borrowed per frame.
//!
//! [`LevelScene`] presents authored levels and runtime entities, and `EnvironmentLighting` converts from the simulation's
//! `EnvironmentFrame`, so games, tools, and previews present the same content the same way.

mod camera;
mod capture;
mod environment;
mod error;
mod form;
mod frame;
mod gpu;
mod level_scene;
mod mesh;
mod pipelines;
mod post;
mod renderer;
mod scene;
mod shadows;

pub use camera::{
    Camera, CameraBasis, CameraFrame, OrbitCamera, WorldRay, intersect_ray_plane, look_at, orthographic, perspective,
    project_to_viewport, screen_ray,
};
pub use capture::{CAPTURE_FORMAT, OffscreenTarget};
pub use error::RenderError;
pub use form::{FormAssetLibrary, compile_form_mesh, form_mesh_asset, primitive_box, primitive_sphere};
pub use frame::{
    CelestialLight, ColorVertex, DEFAULT_AMBIENT_WITHOUT_ENVIRONMENT, EnvironmentLighting, FrameInputs,
    MAX_POINT_LIGHTS, PointLight, sort_transparent_triangles,
};
pub use gpu::{BackendPreference, GpuBackend, GpuCapabilities, GpuContext};
pub use level_scene::{LevelEntity, LevelScene, LevelSceneError};
pub use mesh::{
    AlphaMode, Bounds, DEFAULT_ROUGHNESS, MeshAsset, MeshChannels, MeshData, MeshVertex, Submesh, VertexMaterial,
};
pub use renderer::{Renderer, RendererDiagnostics, RendererOptions};
pub use scene::{MeshCollection, MeshInstance, MeshInstanceId, RenderPassKind, RenderScene, Shading};
pub use shadows::{ShadowOptions, ShadowQuality};

/// The graphics API crate, re-exported for the platform layer. Games use `joy-engine` types instead.
pub use wgpu;
