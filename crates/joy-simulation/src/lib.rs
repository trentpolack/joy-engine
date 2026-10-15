// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Simulation layer of Joy Engine: the environment clock, rigid-body physics, entities, controllers, and authored
//! levels. Everything here is headless and builds identically for native and WebAssembly targets.
//!
//! Conventions shared by every module:
//! - World space is Y-up and right-handed, measured in meters; vehicles drive along local +Z.
//! - Rotations are XYZ Euler radians applied X, then Y, then Z (`joy_foundation::quat_from_euler`).
//! - Time is in seconds. Environment clocks use `f64` like the TypeScript engine; physics and entity updates use
//!   `f32` simulation seconds.
//! - Owners hand out copyable handles and ids (`BodyHandle`, `VehicleHandle`, `EntityId`, `ControllerId`) instead of
//!   long-lived references. Invalid or stale handles are reported, never dereferenced.
//! - Authored data (levels, objects, components) is validated once at the public boundary and reported through typed
//!   errors; nothing in this crate panics on authored content.
//!
//! The physics backend is rapier3d with enhanced determinism. Rapier types never appear in the public API.

pub mod environment;
mod format;
pub mod gameplay;
pub mod physics;

pub use environment::{
    AmbientOcclusionOptions, AtmosphereLight, AtmosphereOptions, CelestialBody, CloudOptions, EnvironmentError,
    EnvironmentFrame, EnvironmentOptions, EnvironmentSystem, LightShaftOptions, Skylight, WindFrame, WindOptions,
};
pub use physics::{
    BODY_DEFAULTS, BodyHandle, BodyOptions, PhysicsError, PhysicsTransform, PhysicsWorld, PhysicsWorldOptions,
    RaycastHit, VEHICLE_DEFAULTS, VehicleHandle, VehicleInput, VehicleOptions, WHEEL_DEFAULTS, WheelOptions,
};
