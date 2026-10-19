// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Rendering layer: the definitive renderer for games, tools, and examples.
//!
//! The renderer draws a retained [`RenderScene`] from a [`Camera`] each frame on wgpu, which runs on WebGPU and WebGL 2
//! in browsers and on Vulkan, Metal, DirectX 12, or OpenGL natively. Shaders are authored once in WGSL.
//!
//! Games and authoring previews share these rendering contracts:
//! - Lit (flat, double-sided vertex-color lighting), PBR (metallic-roughness), and Unlit material passes.
//! - The environment's primary light with cascaded soft shadows, ambient skylight, and up to 32 point lights with
//!   optional soft shadows, on WebGL 2 as well as WebGPU.
//! - The procedural atmosphere sky driven by an environment frame and [`SkyOptions`].
//! - HDR scene targets, bloom, blurs, AgX/ACES/linear tone mapping, grading, vignette, antialiasing, and film grain.
//!
//! The renderer owns every GPU resource it creates; scenes, cameras, and frame inputs are borrowed per frame.
//!
//! The renderer draws meshes, terrain, lights, and particles without knowing where they came from: `joy-scene` turns
//! authored levels, FORM meshes, and per-entity particle effects into scene contents and lights frames from the
//! simulation's `EnvironmentFrame` and the renderer's [`SkyOptions`], so games, tools, and previews present the same
//! content the same way. Textured transparent draws ([`TransparentDraw`], [`TextureAsset`]) carry particle sprites from
//! [`particles`] into the scene pass. Resident continuous-LOD terrain ([`terrain`]) draws through [`TerrainInstance`]s
//! in the same scene, or standalone through [`TerrainRenderer`].

mod camera;
mod capture;
mod display;
mod editor_grid;
mod environment;
mod error;
mod frame;
mod geometry;
mod gpu;
mod materials;
mod mesh;
pub mod particles;
pub mod post_process;
mod postprocessor;
mod renderer;
mod scene;
pub mod scene_effects;
mod shadows;
mod sky;
pub mod terrain;
mod texture;
mod triangle_renderer;

pub use camera::{
    Camera,
    CameraBasis,
    CameraFrame,
    OrbitCamera,
    RegionCamera,
    ShiftedCamera,
    TopDownCamera,
    WorldRay,
    intersect_ray_plane,
    look_at,
    orthographic,
    perspective,
    project_to_viewport,
    screen_ray,
    sort_top_down_transparent_triangles,
    top_down_orthographic,
};
pub use capture::{
    CAPTURE_FORMAT,
    OffscreenTarget,
};
pub use display::{
    BackingLimits,
    backing_pixel_ratio,
};
pub use editor_grid::{
    EditorGrid,
    EditorGridFrame,
    EditorGridView,
};
pub use environment::{
    EnvironmentData,
    EnvironmentEncoding,
    EnvironmentMap,
    EnvironmentMapOptions,
    EnvironmentPixels,
    ImageLighting,
};
pub use error::RenderError;
pub use frame::{
    CelestialLight,
    ColorVertex,
    DEFAULT_AMBIENT_WITHOUT_ENVIRONMENT,
    EnvironmentLighting,
    FrameInputs,
    MAX_POINT_LIGHTS,
    PointLight,
    TexturedVertex,
    TransparentDraw,
    sort_transparent_triangles,
};
pub use geometry::{
    add_circle,
    add_gradient_radial_polygon,
    add_line,
    add_mesh,
    add_polygon,
    add_quad,
    add_radial_polygon,
    add_ring,
    add_triangle,
};
pub use gpu::{
    BackendPreference,
    GpuBackend,
    GpuCapabilities,
    GpuContext,
};
pub use mesh::{
    AlphaMode,
    Bounds,
    DEFAULT_ROUGHNESS,
    MaterialTextures,
    MeshAsset,
    MeshChannels,
    MeshData,
    MeshVertex,
    Submesh,
    TextureMaps,
    VertexMaterial,
};
pub use post_process::{
    PostProcessError,
    PostProcessOverrides,
    PostProcessSettings,
};
pub use renderer::{
    Renderer,
    RendererDiagnostics,
    RendererOptions,
};
pub use scene::{
    DEFAULT_TERRAIN_TOLERANCE,
    MeshCollection,
    MeshInstance,
    MeshInstanceId,
    RenderPassKind,
    RenderScene,
    SceneGraphError,
    SceneNode,
    SceneNodeId,
    Shading,
    TerrainInstance,
    TerrainInstanceId,
};
pub use shadows::{
    ShadowOptions,
    ShadowQuality,
};
pub use sky::{
    AmbientOcclusionOptions,
    AtmosphereOptions,
    CloudOptions,
    LightShaftOptions,
    SkyOptions,
    SkyOptionsError,
};
pub use terrain::{
    TerrainAsset,
    TerrainDiagnostic,
    TerrainError,
    TerrainFrame,
    TerrainRenderer,
    TerrainView,
};
pub use texture::{
    ImageFormat,
    MAX_TEXTURE_DIMENSION,
    TextureAsset,
};
pub use triangle_renderer::{
    TriangleFrame,
    TriangleRenderer,
    TriangleRendererOptions,
};
