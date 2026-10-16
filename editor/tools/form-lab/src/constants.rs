// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM preview display modes, separate from geometry attribute domains.

/// How the preview draws the accepted build.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewMode {
    /// Materials, lighting, and shadows through the engine's PBR pass, plus authored points as markers.
    #[default]
    Shaded,
    /// Every vertex as a point marker, without surfaces.
    Points,
}
