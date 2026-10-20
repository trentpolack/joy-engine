// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/fixed_step.rs`.

use super::*;

#[test]
fn frames_run_whole_fixed_updates_and_keep_the_remainder() {
    let mut fixed_step = FixedStepLoop::default();
    assert_eq!(fixed_step.advance(0.010), 0);
    assert_eq!(fixed_step.advance(0.010), 1);
    assert!((fixed_step.interpolation() - ((0.020 - (1.0 / 60.0)) * 60.0)).abs() < 1e-9);
    // A 30 Hz display runs two updates per frame; a long stall is limited to fifteen.
    fixed_step.reset();
    assert_eq!(fixed_step.advance(1.0 / 30.0), 2);
    fixed_step.reset();
    assert_eq!(fixed_step.advance(5.0), 15);
    assert_eq!(fixed_step.advance(f64::NAN), 0);
    assert_eq!(FixedStepLoop::new(0.0), FixedStepLoop::default());
}
