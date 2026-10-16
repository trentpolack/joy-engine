// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! PARTICLE LAB vocabularies, tuning, and timing values.

/// Preview performance inspection modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PerformanceMode {
    /// No timing.
    Disabled,
    /// Summary timing (the default).
    #[default]
    Overview,
    /// Per-emitter profiling, which adds per-particle timing overhead.
    Detailed,
}

impl PerformanceMode {
    /// Every mode in menu order.
    pub const ALL: [Self; 3] = [Self::Disabled, Self::Overview, Self::Detailed];

    /// Menu label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::Overview => "Overview",
            Self::Detailed => "Detailed profiling",
        }
    }
}

/// Emitter identities referenced by the bundled examples.
pub mod example_emitter {
    /// Ember Impact's ring flash.
    pub const FLASH: &str = "flash";
    /// Ember Impact's streak sparks.
    pub const SPARKS: &str = "sparks";
    /// Ember Impact's fire billow.
    pub const BILLOW: &str = "billow";
    /// Ember Impact's smoke children.
    pub const SMOKE: &str = "smoke";
    /// Solar Swirl's orbiting particles.
    pub const ORBIT: &str = "orbit";
    /// Solar Swirl's core rings.
    pub const CORE: &str = "core";
    /// Solar Swirl's dust trail.
    pub const DUST: &str = "dust";
}

/// Emitter insertion presets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmitterPreset {
    /// Empty scripts and no emission.
    Scratch,
    /// Continuous particles moving outward in 3D.
    Spray,
    /// A short streak burst with gravity.
    Sparks,
    /// Expanding rings for an impact.
    Ring,
    /// Rising soft particles with a gentle fade.
    Smoke,
}

/// Recovery key for the working draft; the TypeScript `joy-games:particle-lab:v1` with periods, because storage keys
/// are portable file names.
pub const STORAGE_KEY: &str = "joy-games.particle-lab.v1";

/// Folder name for native storage.
pub const STORAGE_APPLICATION: &str = "particle-lab";

/// Panel layout preference key (shared spelling with the TypeScript lab).
pub const PANEL_LAYOUT_KEY: &str = "joy-editor-panels-particles-v2";

/// Seconds after the last edit before Live applies it.
pub const LIVE_DELAY_SECONDS: f64 = 0.26;

/// Simulation steps a seek runs per frame, so long seeks never hold a frame.
pub const SEEK_STEPS_PER_FRAME: u32 = 120;

/// Longest replay window and seek, in seconds (the format's hard horizon).
pub const MAXIMUM_REPLAY_SECONDS: f64 = 30.0;

/// Seconds between performance panel refreshes.
pub const PERFORMANCE_REFRESH_SECONDS: f64 = 0.25;

/// Longest real time one frame advances the preview, in seconds, before speed.
pub const MAXIMUM_FRAME_SECONDS: f64 = 0.25;

/// Default preview seed.
pub const DEFAULT_SEED: i64 = 7777;

/// Most particle point lights the preview draws (the renderer's point-light limit).
pub const PREVIEW_LIGHT_LIMIT: usize = 32;

/// Playback speeds offered in the toolbar.
pub const SPEEDS: [(f64, &str); 5] = [(0.25, "¼×"), (0.5, "½×"), (1.0, "1×"), (2.0, "2×"), (4.0, "4×")];
