// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/clock.rs`.

use super::*;

#[test]
fn elapsed_time_is_bounded_and_resets() {
    let mut clock = FrameClock::new(DEFAULT_MAXIMUM_FRAME_SECONDS);
    assert_eq!(clock.tick(10.0).elapsed_seconds, 0.0);
    assert!((clock.tick(10.016).elapsed_seconds - 0.016).abs() < 1e-6);
    assert_eq!(clock.tick(15.0).elapsed_seconds, DEFAULT_MAXIMUM_FRAME_SECONDS);
    clock.reset();
    let resumed = clock.tick(20.0);
    assert_eq!(resumed.elapsed_seconds, 0.0);
    assert_eq!(resumed.frame_index, 3);
    assert!((resumed.time_seconds - 10.0).abs() < 1e-9);
}

#[test]
fn stopwatches_read_monotonic_milliseconds() {
    let stopwatch = Stopwatch::start();
    let first = stopwatch.elapsed_milliseconds();
    let second = stopwatch.elapsed_milliseconds();
    assert!(first >= 0.0 && second >= first);
}

#[test]
fn wall_clock_reads_after_the_epoch_in_whole_milliseconds() {
    let now = unix_time_milliseconds();
    assert!(now > 1.7e12, "the clock reads a current date, got {now}");
    assert_eq!(now.fract(), 0.0);
}
