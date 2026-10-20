// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/profiler.rs`.

use super::*;

/// Each phase runs from its timestamp to the next, the last until the end timestamp; a timestamp written before the
/// previous one (as some drivers reorder empty passes) counts as zero instead of wrapping.
#[test]
fn timestamps_divide_the_frame_into_phases() {
    let phase = |phase| PhaseTiming {
        phase,
        cpu_milliseconds: 0.0,
        gpu_milliseconds: None,
    };
    let mut profile = FrameProfile {
        frame: 3,
        phases: vec![phase(FramePhase::Prepare), phase(FramePhase::Scene), phase(FramePhase::Post)],
        cpu_milliseconds: 1.0,
        gpu_milliseconds: None,
    };
    // Ticks of 1,000 nanoseconds: 2 ms, 0 ms (reordered), then 1.5 ms after the reordered mark.
    apply_gpu_timings(&mut profile, &[1_000, 3_000, 2_500, 4_000], 1_000.0);
    let gpu: Vec<Option<f64>> = profile.phases.iter().map(|timing| timing.gpu_milliseconds).collect();
    assert_eq!(gpu, [Some(2.0), Some(0.0), Some(1.5)]);
    assert_eq!(profile.gpu_milliseconds, Some(3.0));

    // A frame whose readback came up short keeps its CPU timings for the missing phases.
    let mut short = FrameProfile {
        phases: vec![phase(FramePhase::Shadows), phase(FramePhase::Scene)],
        ..profile
    };
    apply_gpu_timings(&mut short, &[0, 1_000], 1_000.0);
    assert_eq!(short.phases[1].gpu_milliseconds, None);
}
