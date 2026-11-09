// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Engine AI building blocks for controllers that decide what an agent does, independent of any entity model.
//!
//! Every Joy game's AI follows the same shape: a controller reads perception from snapshots it cannot change, scores
//! candidates deterministically (often with a lent seeded stream), and emits the same intent or command the player
//! does. This module supplies the shared, deterministic pieces of that shape so games (in Rust or in Lua game scripts)
//! keep only their rules:
//!
//! - [`choice`]: [`BestChoice`], a stable argmin/argmax over scored candidates (first wins ties), with bounds and a
//!   lazily evaluated acceptance test for expensive checks such as line of sight.
//! - [`perception`]: [`nearest`] within a range over 2D or 3D positions, and [`SightCone`] for reticle and seeker
//!   cones.
//! - [`steering`]: turn-limited heading and velocity steering (planar), and turn-limited direction rotation with an
//!   optional seeker cone (3D), for pursuers and homing projectiles.
//!
//! All functions use `f64` and route transcendental math through `math::deterministic`, so decisions replay
//! identically on native and WebAssembly targets. Units are whatever the caller's positions and angles use (angles are
//! radians); nothing here assumes a time step, so callers pass per-tick or per-second limits explicitly.

pub mod choice;
pub mod perception;
pub mod steering;

pub use choice::BestChoice;
pub use perception::{
    ConeSighting,
    Nearest,
    SightCone,
    SpatialPoint,
    nearest,
};
pub use steering::{
    home_direction,
    rotate_direction_toward,
    steer_velocity_toward,
    turn_heading_toward,
};
