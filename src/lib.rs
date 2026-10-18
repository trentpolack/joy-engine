// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Joy Engine: build games and tools once and run them natively (Windows, macOS, Linux) and in browsers (WebGPU or
//! WebGL 2) from the same Rust code.
//!
//! This crate is the facade games and tools depend on. Every engine system has its own crate (in `crates/`), re-exported
//! here under its module name, so `joy_engine::physics` is the `joy-physics` crate; [`rendering`], [`particles`],
//! [`runtime`], and [`development`] gather several crates into one module. The crates follow the dependency layers in
//! `architecture/layers.json`: a crate may use only its own layer and the layers below it, which `npm run check`
//! enforces.
//!
//! | Layer | Modules (crates) | Owns |
//! | --- | --- | --- |
//! | foundation | [`core`] (`joy-core`), [`scripting`] (`joy-core`) | Math, transforms, colors, seeded random, primitive shapes, display-processing profiles, script language information |
//! | simulation | [`environment`] (`joy-environment`), [`physics`] (`joy-physics`), [`gameplay`] (`joy-gameplay`) | Solar time and wind, rigid bodies and vehicles, entities, controllers, levels |
//! | content | [`dcc`] (`joy-dcc-gltf`), [`form`] (`joy-form`), [`particles`] (`joy-fx`), [`project`] (`joy-project`) | GLB and glTF mesh import, the FORM procedural modeling language (renderer-free geometry), `.joyfx` particle documents, scripts, and simulation, project manifests |
//! | rendering | [`rendering`] (`joy-renderer`, `joy-scene`) | The wgpu renderer every game, tool, and preview draws with, including particle drawing, and the scene presentation of levels, FORM meshes, and effects |
//! | platform | [`runtime`] (`joy-app`, `joy-input`, `joy-audio`, `joy-storage`, `joy-ui`), [`development`] (`joy-devtools`, `joy-authoring`) | The game shell, frame phases, input and gamepads, audio, storage, egui UI and theme, dev tools, the authoring kit, browser glue |
//!
//! Conventions shared by every module:
//! - World space is Y-up and right-handed, measured in meters; vehicles drive along local +Z.
//! - Rotations are XYZ Euler radians applied X, then Y, then Z ([`core::quat_from_euler`]).
//! - Time is in seconds. Environment clocks use `f64`; physics and entity updates use `f32`.
//! - Owners hand out copyable handles and ids (`BodyHandle`, `EntityId`, `MeshInstanceId`) instead of long-lived
//!   references; stale handles are reported, never dereferenced.
//! - Authored data is validated once at the public boundary and reported through typed errors; authored content never
//!   causes a panic.
//!
//! Games and tools depend on this crate only, never on wgpu, winit, rapier, or egui directly (egui is re-exported for
//! UI). A game implements [`Game`] and hands it to [`run`]:
//!
//! ```no_run
//! use joy_engine::prelude::*;
//!
//! struct Spinner { scene: RenderScene, camera: OrbitCamera, cube: Option<MeshInstanceId> }
//!
//! impl Game for Spinner {
//!     fn start(&mut self, _context: &mut StartContext<'_>) -> GameResult {
//!         self.cube = Some(self.scene.create_mesh(primitive_box()));
//!         Ok(())
//!     }
//!
//!     fn simulation(&mut self, frame: &mut Frame<'_>) -> GameResult {
//!         if let Some(cube) = self.cube.and_then(|id| self.scene.get_mut(id)) {
//!             cube.transform.rotation.y += frame.time.elapsed_seconds;
//!         }
//!         Ok(())
//!     }
//!
//!     fn render(&mut self, _frame: &mut Frame<'_>, target: &mut RenderTarget<'_>) -> GameResult {
//!         Ok(target.draw(&self.scene, &self.camera, &FrameInputs::default())?)
//!     }
//! }
//!
//! fn main() -> Result<(), ShellError> {
//!     let game = Spinner { scene: RenderScene::new(), camera: OrbitCamera { distance: 4.0, ..Default::default() }, cube: None };
//!     run(ShellOptions { title: "Spinner".into(), ..Default::default() }, game)
//! }
//! ```
//!
//! Browser builds compile a game crate as a `cdylib` for `wasm32-unknown-unknown`; [`web_main!`] declares the page
//! entry point, and `scripts/rust/build-web.mjs` produces the deployable page.

// Foundation.
#[doc(inline)]
pub use joy_core as core;
#[doc(inline)]
pub use joy_core::scripting;

// Simulation.
#[doc(inline)]
pub use joy_environment as environment;
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

/// Particle effects (`.joyfx`): documents, scripts, and deterministic simulation from `joy-fx`, plus the renderer's
/// particle drawing (lights, camera-facing triangles, and textures), in one place for games and tools.
pub mod particles {
    #[doc(inline)]
    pub use joy_fx::*;
    #[doc(inline)]
    pub use joy_renderer::particles::*;
}

// Platform.
pub mod development;
pub mod runtime;

pub use development::DevTools;
pub use runtime::{
    AudioBank,
    Frame,
    FrameTime,
    Game,
    GameError,
    GameResult,
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
};

/// Immediate-mode UI for HUDs and tools.
pub use egui;
/// Vector math used throughout the engine's public types.
pub use glam;
/// Logging facade; the shell routes it to the terminal natively and to the browser console on the web.
pub use log;
/// JSON values for authored records and FORM parameters.
pub use serde_json;

/// The browser binding generator, re-exported for [`web_main!`]. Not part of the game-facing API.
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub use wasm_bindgen;

/// The names most games use.
pub mod prelude {
    pub use crate::core::{
        PostProcessingConfig,
        PostProcessingProfile,
        RandomSource,
        Rgb,
        Rgba,
        SeededRandom,
        Transform,
    };
    pub use crate::development::DevTools;
    pub use crate::environment::{
        EnvironmentFrame,
        EnvironmentOptions,
        EnvironmentSystem,
    };
    pub use crate::gameplay::{
        ControlIntent,
        EntityBehavior,
        EntityDefinition,
        EntityId,
        EntityRegistry,
        EntityState,
        IntentController,
        LevelData,
        LevelRuntime,
        SimulationContext,
        parse_level,
    };
    pub use crate::particles::{
        CompiledParticleEffect,
        PARTICLE_STEP_SECONDS,
        PARTICLE_XY_BASIS,
        ParticleCameraBasis,
        ParticleEffect,
        ParticleEffectOptions,
        ParticleTextureSet,
        append_particle_effect,
        collect_particle_lights,
        compile_particle_effect,
        create_particle_effect_batches,
        parse_particle_effect,
    };
    pub use crate::physics::{
        PhysicsWorld,
        PhysicsWorldOptions,
        VehicleInput,
        VehicleTuning,
    };
    pub use crate::rendering::{
        AssetScene,
        BackendPreference,
        Camera,
        EnvironmentLighting,
        FormAssetLibrary,
        FrameInputs,
        LevelScene,
        MeshAsset,
        MeshCollection,
        MeshInstance,
        MeshInstanceId,
        OrbitCamera,
        PointLight,
        RenderScene,
        RendererOptions,
        Shading,
        ShadowOptions,
        ShadowQuality,
        SkyOptions,
        TextureAsset,
        TransparentDraw,
        compile_form_mesh,
        environment_lighting,
        primitive_box,
        primitive_sphere,
    };
    pub use crate::runtime::{
        AudioBank,
        AudioBankOptions,
        FixedStepLoop,
        Frame,
        FrameTime,
        Game,
        GameError,
        GameResult,
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
    pub use glam::{
        Mat4,
        Quat,
        Vec2,
        Vec3,
        Vec4,
    };
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
