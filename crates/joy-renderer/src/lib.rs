// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Rendering layer: the definitive renderer for games, tools, and examples.
//!
//! The renderer draws a retained [`RenderScene`] from a [`Camera`] each frame on wgpu, which runs on WebGPU and WebGL 2
//! in browsers and on Vulkan, Metal, DirectX 12, or OpenGL natively. Shaders are authored once in WGSL.
//!
//! Games and authoring previews share these rendering contracts:
//! - Materials ([`MaterialAsset`]) drawn by registered shading models: lit (flat, faceted, double-sided stylized
//!   lighting), layered (physically based surface layers), unlit, and any model a game registers. Meshes carry
//!   geometry and material slots only; instances may override any slot's material.
//! - A directional key light with cascaded soft shadows, a sky-and-ground skylight, and any number of point lights (up
//!   to [`MAX_POINT_LIGHTS`] that matter most to each view draw, binned into light clusters, with optional soft
//!   shadows from a shared atlas). Opaque surfaces shade deferred where the device allows and forward on WebGL 2.
//! - The procedural atmosphere sky driven by an environment frame and [`SkyOptions`].
//! - HDR scene targets, scene effects, bloom, and filmic (Ushimura/GT-style curve) or AgX (or no tonemapping).
//!
//! The renderer owns every GPU resource it creates; scenes, cameras, and frame inputs are borrowed per frame.
//!
//! The renderer draws meshes, terrain, lights, and particles without knowing where they came from: `joy-scene` turns
//! authored levels, FORM meshes, and per-entity particle effects into scene contents and lights frames from the
//! simulation's `EnvironmentFrame` and the renderer's [`SkyOptions`], so games, tools, and previews present the same
//! content the same way. Textured transparent draws ([`TransparentDraw`], [`TextureAsset`]) carry particle sprites from
//! [`particles`] into the scene pass. Paged continuous-LOD terrain ([`terrain`]) draws `joy_terrain` surfaces through
//! [`TerrainInstance`]s in the same scene.

mod camera;
mod capture;
mod culling;
mod debug_view;
mod deferred;
mod display;
mod display_output;
mod editor_grid;
mod environment;
mod error;
mod frame;
mod geometry;
mod gpu;
mod lighting;
mod material;
mod mesh;
pub mod particles;
mod pipelines;
mod point_lights;
pub mod post_process;
mod postprocessor;
mod profiler;
mod renderer;
mod scene;
pub mod scene_effects;
mod shadows;
mod sky;
pub mod terrain;
mod texture;
mod texture_mips;
mod transparency;
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
pub use debug_view::DebugView;
pub use display::{
    BackingLimits,
    backing_pixel_ratio,
};
pub use display_output::{
    DisplayOutput,
    MAX_DISPLAY_HEADROOM,
    OutputEncoding,
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
    ColorVertex,
    DEFAULT_AMBIENT_WITHOUT_ENVIRONMENT,
    EnvironmentLighting,
    FrameInputs,
    MAX_POINT_LIGHTS,
    PresentationTime,
    TexturedVertex,
    TransparentDraw,
};
pub use geometry::{
    ShadedShape,
    ShadedTriangle,
    add_circle,
    add_gradient_radial_polygon,
    add_line,
    add_polygon,
    add_quad,
    add_radial_polygon,
    add_ring,
    add_shaded_shape,
    add_triangle,
};
pub use gpu::{
    BackendPreference,
    GpuBackend,
    GpuCapabilities,
    GpuContext,
};
/// The shared render records, re-exported so rendering code names lights, meshes, and materials beside the assets and
/// frame inputs that carry them.
pub use joy_render_types::{
    AlphaMode,
    Coat,
    CustomShading,
    DirectionalLight,
    Fuzz,
    Haze,
    LitSurface,
    Material,
    MaterialSlot,
    Mesh,
    MeshDocument,
    PointLight,
    ShaderValue,
    Shading,
    ShadingModel,
    Skylight,
    Surface,
    SurfaceLayer,
    SurfaceMaps,
    ThinFilm,
    UnlitSurface,
};
pub use material::{
    MaterialAsset,
    ParameterDefinition,
    ShadingModelDefinition,
    TextureDefinition,
    TextureFallback,
};
pub use mesh::{
    Bounds,
    MeshAsset,
    Submesh,
};
pub use post_process::{
    PostProcessError,
    PostProcessOverrides,
    PostProcessSettings,
};
pub use profiler::{
    FramePhase,
    FrameProfile,
    PhaseTiming,
};
pub use renderer::{
    MAX_RENDER_SCALE,
    MIN_RENDER_SCALE,
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
    TerrainInstance,
    TerrainInstanceId,
};
pub use shadows::{
    MAX_SHADOWED_POINT_LIGHTS,
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
    TerrainDiagnostic,
    TerrainStats,
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
