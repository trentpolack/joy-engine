// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The development hooks every build keeps: the overlay the shell hands each frame ([`Frame::dev_tools`](crate::Frame))
//! and the room the development dock takes at the bottom of the screen ([`development_dock_height`]).
//!
//! With the `devtools` feature these are `joy-devtools`' overlay and dock: statistics, the console, and the CONFIG and
//! JOY-EDITOR launchers. Without it, which is how release and browser production builds compile, `DevTools` is an inert
//! stand-in that stores and draws nothing and the dock takes no room, so games publish diagnostics and lay out their
//! HUDs without `#[cfg]` and production binaries carry no development UI. Anything beyond publishing values (the
//! console, editors, and dock widgets) is reached through `joy_engine::development`, which exists only with the
//! feature.

#[cfg(feature = "devtools")]
pub use joy_devtools::DevTools;
#[cfg(feature = "devtools")]
pub(crate) use joy_devtools::dock;

#[cfg(not(feature = "devtools"))]
pub use production::DevTools;
#[cfg(not(feature = "devtools"))]
pub(crate) use production::dock;

/// Height in logical pixels the development dock occupies above the bottom edge this frame, so HUD elements in the
/// bottom-right corner can sit above it: zero when no launcher is showing, and always zero in builds without the
/// development tools.
pub fn development_dock_height(context: &egui::Context) -> f32 {
    dock::reserved_height(context)
}

/// Production builds' development hooks: the same calls the shell and games make, doing nothing.
#[cfg(not(feature = "devtools"))]
mod production {
    use joy_renderer::{
        FrameProfile,
        Renderer,
        RendererDiagnostics,
    };
    use joy_ui::Viewport;

    /// The development overlay of a build without development tools: never enabled, and published values are dropped.
    pub struct DevTools {
        // Constructed only by the shell.
        _private: (),
    }

    impl DevTools {
        /// The overlay the shell creates; development flags have nothing to enable here.
        pub(crate) fn new(_title: &str, _enabled: bool) -> Self {
            Self {
                _private: (),
            }
        }

        /// Always false: this build has no development overlay.
        pub fn enabled(&self) -> bool {
            false
        }

        /// Accept a diagnostic value for the overlay this build does not have.
        pub fn set(&mut self, _key: &str, _value: impl Into<String>) {}

        /// Development messages stay silent in production.
        pub fn log(&self, _message: &str) {}

        pub(crate) fn toggle_console(&mut self) {}

        pub(crate) fn apply_to_renderer(&mut self, _renderer: &mut Renderer) {}

        pub(crate) fn ui(
            &mut self,
            _context: &egui::Context,
            _diagnostics: &RendererDiagnostics,
            _profile: Option<&FrameProfile>,
            _viewport: Viewport,
        ) {
        }

        pub(crate) fn frame(&mut self, _elapsed_seconds: f32) {}
    }

    /// The dock queries the shell and HUDs make; with no dock, nothing is open, claimed, or reserved.
    pub(crate) mod dock {
        pub(crate) fn reserved_height(_context: &egui::Context) -> f32 {
            0.0
        }

        pub(crate) fn close_open_panel(_context: &egui::Context) -> bool {
            false
        }

        pub(crate) fn key_claimed(_context: &egui::Context, _key: &str) -> bool {
            false
        }
    }
}
