// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Joy Engine: build games and tools once and run them natively (Windows, macOS, Linux) and in browsers (WebGPU or
//! WebGL 2) from the same Rust code.
//!
//! Games depend on this crate only. It re-exports the engine layers, each also usable alone:
//!
//! | Module | Crate | Owns |
//! | --- | --- | --- |
//! | [`foundation`] | `joy-foundation` | Math, transforms, colors, seeded random, display-processing profiles |
//! | [`simulation`] | `joy-simulation` | Environment clock, rigid bodies and vehicles, entities, controllers, levels |
//! | [`form`] | `joy-form` | The FORM procedural modeling language (renderer-free geometry) |
//! | [`render`] | `joy-render` | The wgpu renderer every game, tool, and preview draws with |
//! | [`platform`] | `joy-platform` | The game shell: window or canvas, frame phases, input, egui HUD and dev tools |
//!
//! A game implements [`Game`] and hands it to [`run`]:
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
//! Browser builds compile the same crate as a `cdylib` for `wasm32-unknown-unknown`; [`web_main!`] declares the page
//! entry point, and `scripts/rust/build-web.mjs` in the engine produces the deployable page.

pub use joy_form as form;
pub use joy_foundation as foundation;
pub use joy_platform as platform;
pub use joy_render as render;
pub use joy_simulation as simulation;

pub use joy_platform::{
    DevTools, Frame, FrameTime, Game, GameError, GameResult, InputBinding, InputManager, InputMap, RenderTarget,
    ShellError, ShellOptions, StartContext, Viewport, egui, key, run, theme,
};
#[cfg(not(target_arch = "wasm32"))]
pub use joy_platform::{HeadlessOptions, HeadlessShell};

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
    pub use glam::{Mat4, Quat, Vec2, Vec3, Vec4};
    pub use joy_foundation::{
        PostProcessingConfig, PostProcessingProfile, RandomSource, Rgb, Rgba, SeededRandom, Transform,
    };
    pub use joy_platform::{
        DevTools, Frame, FrameTime, Game, GameError, GameResult, InputBinding, InputManager, InputMap, RenderTarget,
        ShellError, ShellOptions, StartContext, Viewport, egui, key, run,
    };
    pub use joy_render::{
        BackendPreference, Camera, EnvironmentLighting, FormAssetLibrary, FrameInputs, LevelScene, MeshAsset,
        MeshCollection, MeshInstance, MeshInstanceId, OrbitCamera, PointLight, RenderScene, RendererOptions, Shading,
        ShadowOptions, ShadowQuality, compile_form_mesh, primitive_box, primitive_sphere,
    };
    pub use joy_simulation::gameplay::{
        ControlIntent, EntityBehavior, EntityDefinition, EntityId, EntityRegistry, EntityState, IntentController,
        LevelData, LevelRuntime, SimulationContext, parse_level,
    };
    pub use joy_simulation::{
        EnvironmentFrame, EnvironmentOptions, EnvironmentSystem, PhysicsWorld, PhysicsWorldOptions, VehicleInput,
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
