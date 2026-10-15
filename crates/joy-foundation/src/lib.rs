// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Foundation layer of Joy Engine: shared data and algorithms with no simulation, content, rendering, or platform
//! dependencies.
//!
//! Coordinate conventions used across the engine:
//! - World space is Y-up and right-handed, measured in meters (or project world units).
//! - Rotations are XYZ Euler radians applied X, then Y, then Z (the matrix `Rz * Ry * Rx`).
//! - Transforms apply scale, then rotation, then translation.
//! - Colors are linear RGB(A) unless a function says otherwise.

pub mod color;
pub mod math;
pub mod post_processing;
pub mod primitive;
pub mod random;
pub mod transform;

pub use color::{Rgb, Rgba, parse_hex_color, with_alpha};
pub use glam;
pub use math::{TAU, clamp, distance_to_rectangle_edge, random_integer, random_range, smoothstep};
pub use post_processing::{PostProcessingConfig, PostProcessingError, PostProcessingProfile};
pub use primitive::PrimitiveShape;
pub use random::{RandomSource, SeededRandom};
pub use transform::{Transform, euler_from_quat, quat_from_euler};

/// Free-form JSON record used for authored component data that the engine preserves without interpreting.
pub type JsonRecord = serde_json::Map<String, serde_json::Value>;
