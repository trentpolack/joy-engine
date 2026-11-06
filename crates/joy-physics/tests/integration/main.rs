// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Integration tests for `joy-physics`: rigid bodies, vehicles, SDF contacts, terrain colliders, destruction
//! recordings, and structural stress and undermining. Run with `cargo test -p joy-physics`; no GPU or window stack is
//! built.

mod destruction;
mod destruction_fracture;
mod destruction_stress;
mod physics;
mod terrain_colliders;
