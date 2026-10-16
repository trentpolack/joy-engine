// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Level editor tools, viewport presentation, and camera defaults.

/// Viewport manipulation tools.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LevelTool {
    /// Click an entity to select it; drag to orbit.
    #[default]
    Select,
    /// Drag a gizmo handle, or click the ground, to move the selection. Height is preserved by ground clicks.
    Move,
}

/// Linear background behind the level.
pub const CLEAR_COLOR: [f32; 4] = [0.045, 0.065, 0.10, 1.0];

/// Ambient irradiance of the viewport, which draws without an environment.
pub const VIEWPORT_AMBIENT: f32 = 0.65;

/// Initial camera distance in meters.
pub const DEFAULT_CAMERA_DISTANCE: f32 = 26.0;

/// Initial camera pitch in radians.
pub const DEFAULT_CAMERA_PITCH: f32 = 0.7;

/// Initial camera yaw in radians.
pub const DEFAULT_CAMERA_YAW: f32 = 0.75;

/// Gizmo axis length as a fraction of the camera distance, so it keeps a steady size on screen.
pub const GIZMO_SCREEN_FRACTION: f32 = 0.12;

/// How close, in logical pixels, a press must land to a gizmo axis to grab it.
pub const GIZMO_HIT_PIXELS: f32 = 8.0;

/// Color of newly added primitives (linear RGB).
pub const PRIMITIVE_COLOR: [f64; 3] = [0.95, 0.36, 0.12];

/// Name prompted for a new custom component.
pub const DEFAULT_CUSTOM_COMPONENT: &str = "gameplay";
