// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/convert.rs`.

use super::*;

#[test]
fn solver_rotations_match_the_engine_euler_convention() {
    for rotation in [Vec3::new(0.3, -0.7, 1.1), Vec3::new(-1.2, 2.2, -2.8), Vec3::new(0.6, 1.5707964, -0.3)] {
        let solver = solver_rotation_from_euler(rotation);
        let engine = joy_core::quat_from_euler(rotation);
        let difference = Quat::from_xyzw(solver.x, solver.y, solver.z, solver.w).dot(engine).abs();
        assert!((difference - 1.0).abs() < 1e-6, "{rotation}");
    }
}
