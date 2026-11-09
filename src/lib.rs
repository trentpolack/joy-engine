// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Joy Engine: build games and tools once and run them natively (Windows, macOS, Linux) and in browsers (WebGPU or
//! WebGL 2) from the same Rust code.
//!
//! This crate is the facade games and tools depend on. Every engine system has its own crate (in `crates/`), re-exported
//! here under its module name, so `joy_engine::physics` is the `joy-physics` crate; [`rendering`], [`particles`],
//! [`terrain`], [`runtime`], and [`development`] gather several crates into one module. [`math`] is the engine's vector, matrix,
//! and quaternion vocabulary (`Vec3` for rendering, `DVec3` for seeded simulation) with its deterministic functions.
//! The crates follow the dependency layers named in their `Cargo.toml`s: a crate may use only its own layer and the
//! layers below it, which `npm run check` enforces.
//!
//! | Layer | Modules (crates) | Owns |
//! | --- | --- | --- |
//! | foundation | [`core`] (`joy-core`), [`scripting`] (`joy-core`), [`render_types`] (`joy-render-types`) | Math, transforms, colors, seeded random, primitive shapes, script language information; GPU-free meshes, materials, and lights |
//! | simulation | [`environment`] (`joy-environment`), [`terrain`] (`joy-terrain`), [`physics`] (`joy-physics`), [`gameplay`] (`joy-gameplay`), `game_script` (`joy-game-script`) | Solar time and wind, terrain heightfields, tile pyramids, streaming, detail, and craters, rigid bodies, entities, controllers, levels, sandboxed Lua game scripts (`game-script` feature) |
//! | content | [`dcc`] (`joy-dcc-gltf`), [`form`] (`joy-form`), [`particles`] (`joy-fx`), [`project`] (`joy-project`) | GLB and glTF mesh import, the FORM procedural modeling language (renderer-free geometry), `.joyfx` particle documents, scripts, and simulation, project manifests |
//! | rendering | [`rendering`] (`joy-renderer`, `joy-scene`) | The wgpu renderer every game, tool, and preview draws with, including particle drawing, and the scene presentation of levels, FORM meshes, and effects |
//! | platform | [`runtime`] (`joy-app`, `joy-input`, `joy-audio`, `joy-storage`, `joy-ui`), [`development`] (`joy-devtools`, `joy-authoring`) | The game shell, frame phases, embedded game assets, input and gamepads, audio, storage, egui UI and theme, dev tools, the authoring kit, browser glue |
//!
//! Cargo features decide what a build ships:
//! - `devtools` (default): [`development`]'s dock (the CONFIG and JOY-EDITOR menus), statistics overlay, console, config
//!   panels, live tuning, and effect editors. Without it the shell compiles no development UI; [`Frame::dev_tools`] is an
//!   inert [`DevTools`] that accepts published values, and [`runtime::development_dock_height`] is zero, so game code
//!   needs `#[cfg(feature = "devtools")]` only around the editors it attaches.
//! - `authoring`: the authoring kit, native file dialogs, and `project` manifests behind Joy Editor, FORM EDITOR, and
//!   JOYFX EDITOR (implies `devtools`).
//! - `game-script`: sandboxed Lua game scripts (`game_script`). `test-support`: golden-file helpers (`testing`).
//!
//! Games depend on the facade with `default-features = false` and turn `devtools` on through a default feature of their
//! own, which production builds drop (`--no-default-features`), so release and browser builds contain no development or
//! authoring code. Those builds also load FORM meshes their build script cooked ([`cooked_forms!`], `joy-cook`) instead
//! of evaluating FORM, while native development builds evaluate live and hot reload ([`game_assets!`]).
//!
//! Conventions shared by every module:
//! - World space is Y-up and right-handed, measured in meters; +Z is an object's local forward.
//! - Rotations are XYZ Euler radians applied X, then Y, then Z ([`core::quat_from_euler`]).
//! - Time is in seconds. Environment clocks use `f64`; physics and entity updates use `f32`.
//! - Owners hand out copyable handles and ids (`BodyHandle`, `EntityId`, `MeshInstanceId`) instead of long-lived
//!   references; stale handles are reported, never dereferenced.
//! - Authored data is validated once at the public boundary and reported through typed errors; authored content never
//!   causes a panic.
//!
//! Games and tools depend on this crate only, never on wgpu, winit, rapier, or egui directly (egui is re-exported for
//! UI). A game, tool, or example implements [`App`] and hands it to [`run`]:
//!
//! ```no_run
//! use joy_engine::prelude::*;
//!
//! struct Spinner { scene: RenderScene, camera: OrbitCamera, cube: Option<MeshInstanceId> }
//!
//! impl App for Spinner {
//!     fn start(&mut self, _context: &mut StartContext<'_>) -> AppResult {
//!         self.cube = Some(self.scene.create_mesh(primitive_box()));
//!         Ok(())
//!     }
//!
//!     fn simulation(&mut self, frame: &mut Frame<'_>) -> AppResult {
//!         if let Some(cube) = self.cube.and_then(|id| self.scene.get_mut(id)) {
//!             cube.transform.rotation.y += frame.time.elapsed_seconds;
//!         }
//!         Ok(())
//!     }
//!
//!     fn render(&mut self, _frame: &mut Frame<'_>, target: &mut RenderTarget<'_>) -> AppResult {
//!         Ok(target.draw(&self.scene, &self.camera, &FrameInputs::default())?)
//!     }
//! }
//!
//! fn main() -> Result<(), ShellError> {
//!     let app = Spinner { scene: RenderScene::new(), camera: OrbitCamera { distance: 4.0, ..Default::default() }, cube: None };
//!     run(ShellOptions { title: "Spinner".into(), ..Default::default() }, app)
//! }
//! ```
//!
//! Browser builds compile a game crate as a `cdylib` for `wasm32-unknown-unknown`; [`web_main!`] declares the page
//! entry point, and `scripts/rust/build-web.mjs` produces the deployable page.

// Foundation.
#[doc(inline)]
pub use joy_core as core;
#[doc(inline)]
pub use joy_core::math;
#[doc(inline)]
pub use joy_core::scripting;
#[doc(inline)]
pub use joy_render_types as render_types;

// Simulation.
#[doc(inline)]
pub use joy_environment as environment;
#[doc(inline)]
#[cfg(feature = "game-script")]
pub use joy_game_script as game_script;
#[doc(inline)]
pub use joy_gameplay as gameplay;
#[doc(inline)]
pub use joy_physics as physics;

// Content.
#[doc(inline)]
pub use joy_dcc_gltf as dcc;
#[doc(inline)]
pub use joy_form as form;
#[doc(inline)]
#[cfg(feature = "authoring")]
pub use joy_project as project;

// Rendering.
/// The renderer every game, tool, and preview draws with (`joy-renderer`), plus scene presentation for levels, FORM
/// meshes, and per-entity particle effects (`joy-scene`), in one place for games and tools.
pub mod rendering {
    #[doc(inline)]
    pub use joy_renderer::*;
    #[doc(inline)]
    pub use joy_scene::*;
}

/// Terrain (`joy-terrain`): authored heightfields, paged tile pyramids and packages, streaming, quadtree LOD
/// selection, synthesized detail, craters, and gameplay sampling, plus the renderer's terrain diagnostics and
/// statistics, in one place for games and tools. Draw a surface with a `rendering::TerrainInstance`; collide with it
/// through `physics::TerrainColliders`.
pub mod terrain {
    #[doc(inline)]
    pub use joy_renderer::terrain::*;
    #[doc(inline)]
    pub use joy_terrain::*;
}

/// Particle effects (`.joyfx`): documents, scripts, and deterministic simulation from `joy-fx`, plus the renderer's
/// particle drawing (lights, GPU-instanced particles submitted through a `ParticleFrame`, and textures), in one place for
/// games and tools.
pub mod particles {
    #[doc(inline)]
    pub use joy_fx::*;
    #[doc(inline)]
    pub use joy_renderer::particles::*;
}

// Platform.
#[cfg(feature = "devtools")]
pub mod development;
pub mod runtime;

pub use joy_app::{
    game_assets,
    source_root,
};
pub use joy_form::cooked_forms;
pub use runtime::{
    App,
    AppError,
    AppResult,
    AudioBank,
    DevTools,
    FixedTick,
    Frame,
    FrameTime,
    GameAssets,
    InputBinding,
    InputManager,
    InputMap,
    RenderTarget,
    ShellError,
    ShellOptions,
    StartContext,
    Viewport,
    key,
    run,
    theme,
};
#[cfg(not(target_arch = "wasm32"))]
pub use runtime::{
    HeadlessOptions,
    HeadlessShell,
    region_mean_color,
};

/// Immediate-mode UI for HUDs and tools.
pub use egui;
/// Logging facade; the shell routes it to the terminal natively and to the browser console on the web.
pub use log;
/// JSON values for authored records and FORM parameters.
pub use serde_json;

/// Golden-file helpers for games' tests (`joy-test-support`): recordings of deterministic behavior that tests check
/// and re-record with `JOY_UPDATE_FIXTURES=1`. Enabled by the `test-support` feature, which games turn on only in their
/// `[dev-dependencies]`.
#[cfg(feature = "test-support")]
pub use joy_test_support as testing;
/// The browser binding generator, re-exported for [`web_main!`]. Not part of the game-facing API.
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub use wasm_bindgen;

/// The names most games use.
pub mod prelude {
    pub use crate::core::{
        Color,
        EventChannel,
        EventCursor,
        EventQueue,
        FixedStepLoop,
        LinearColor,
        RandomSource,
        SeededRandom,
        Transform,
    };
    pub use crate::environment::{
        EnvironmentFrame,
        EnvironmentOptions,
        EnvironmentSystem,
    };
    pub use crate::form::CookedForms;
    pub use crate::gameplay::{
        ComponentRegistry,
        ContactEvent,
        ControlIntent,
        EntityBehavior,
        EntityComponent,
        EntityDefinition,
        EntityId,
        EntityRegistry,
        EntityRemoved,
        EntitySpawned,
        EntityState,
        EntitySystem,
        IntentController,
        LevelData,
        LevelRuntime,
        PresentedEntity,
        SimulationContext,
        TimerFired,
        TimerRepeat,
        World,
        parse_level,
    };
    pub use crate::math::{
        DVec2,
        DVec3,
        Mat4,
        Quat,
        Vec2,
        Vec3,
        Vec4,
    };
    pub use crate::particles::{
        CompiledParticleEffect,
        EffectPoolOptions,
        PARTICLE_STEP_SECONDS,
        ParticleEffect,
        ParticleEffectOptions,
        ParticleEffectPool,
        ParticleFrame,
        ParticleTextureSet,
        collect_particle_lights,
        compile_particle_effect,
        parse_particle_effect,
    };
    pub use crate::physics::{
        BodyOptions,
        CollisionGroups,
        ContactPhase,
        PhysicsSimulation,
        PhysicsSimulationOptions,
        QueryOptions,
    };
    pub use crate::rendering::{
        AssetScene,
        BackendPreference,
        Camera,
        EnvironmentLighting,
        FormAssetLibrary,
        FrameInputs,
        LevelScene,
        Material,
        MaterialAsset,
        Mesh,
        MeshAsset,
        MeshCollection,
        MeshInstance,
        MeshInstanceId,
        OrbitCamera,
        PointLight,
        PostProcessSettings,
        PostProcessVolumes,
        RenderScene,
        RendererOptions,
        Shading,
        ShadingModel,
        ShadowOptions,
        ShadowQuality,
        SkyOptions,
        TextureAsset,
        compile_cooked_form_mesh,
        compile_form_mesh,
        environment_lighting,
        primitive_box,
        primitive_sphere,
    };
    pub use crate::runtime::{
        App,
        AppError,
        AppResult,
        AudioBank,
        AudioBankOptions,
        DevTools,
        FixedTick,
        Frame,
        FrameTime,
        GameAssets,
        InputAction,
        InputAxis,
        InputBinding,
        InputManager,
        InputMap,
        PcmSound,
        RenderTarget,
        ShellError,
        ShellOptions,
        SoundDefinition,
        StartContext,
        Viewport,
        gamepad,
        key,
        run,
    };
    pub use egui;
}

/// Declare `$run` (a `fn()`) as the browser entry point when compiling for WebAssembly; expands to nothing natively.
///
/// Use it in a library crate built as a `cdylib`, or in a binary whose native `main` calls `$run` only when not
/// compiling for WebAssembly, so the page starts the game exactly once.
///
/// ```ignore
/// fn start() {
///     if let Err(error) = joy_engine::run(options(), MyGame::new()) {
///         joy_engine::log::error!("{error}");
///     }
/// }
/// joy_engine::web_main!(start);
/// ```
#[macro_export]
macro_rules! web_main {
    ($run:path) => {
        /// Browser entry point generated by `joy_engine::web_main!`.
        #[cfg(target_arch = "wasm32")]
        #[$crate::wasm_bindgen::prelude::wasm_bindgen(start, wasm_bindgen = $crate::wasm_bindgen)]
        pub fn __joy_engine_web_start() {
            $run();
        }
    };
}
