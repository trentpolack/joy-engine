// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Interaction tuning shared by editor viewports and property controls.

/// Camera orbit per dragged logical pixel, in radians.
pub const ORBIT_RADIANS_PER_PIXEL: f32 = 0.008;

/// Camera pan per dragged logical pixel, as a fraction of the camera distance.
pub const PAN_SPEED_PER_DISTANCE: f32 = 0.0016;

/// Lowest camera pitch above the horizon, in radians.
pub const MINIMUM_PITCH: f32 = 0.08;

/// Highest camera pitch, in radians.
pub const MAXIMUM_PITCH: f32 = 1.5;
