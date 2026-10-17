// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The headless benchmark: both bins fill with the same props on the same schedule, without a renderer, and report
//! their CPU physics time per fixed step and a hash of the final pile.
//!
//! Natively `--bench` prints the report and exits; in browsers `?bench=1` logs it to the console and shows it in the
//! HUD. Comparing the two hashes across targets checks that SDF contact generation is deterministic there too.

use std::fmt;

use joy_engine::physics::{PhysicsError, SdfCollisionStatistics};
use joy_engine::runtime::Stopwatch;

use super::config::sdf_collisions_config::PHYSICS;
use super::pile_arena::{ArenaSnapshot, ColliderMode, PileArena};
use super::prop_library::PropLibrary;

/// Final steps averaged for the settled-pile timing.
const SETTLED_STEPS: u32 = 120;

/// One bin's benchmark result.
#[derive(Clone, Debug, PartialEq)]
pub struct BinReport {
    /// CPU milliseconds per fixed step, averaged over the whole run (drops, impacts, and settling).
    pub milliseconds_per_step: f64,
    /// CPU milliseconds per fixed step over the final steps, once the pile has (mostly) come to rest.
    pub settled_milliseconds_per_step: f64,
    /// Position hash of the final pile.
    pub hash: u64,
    /// The final pile's diagnostics.
    pub snapshot: ArenaSnapshot,
}

/// Both bins' results for the same scenario.
#[derive(Clone, Debug, PartialEq)]
pub struct BenchmarkReport {
    /// Fixed steps simulated per bin.
    pub steps: u32,
    /// Props dropped into each bin.
    pub props: usize,
    /// The SDF bin and the bounding-box bin.
    pub sdf: BinReport,
    pub boxes: BinReport,
}

/// Fill both bins with `props` props over `steps` fixed steps and time them separately.
pub fn run_benchmark(library: &PropLibrary, steps: u32, props: usize) -> Result<BenchmarkReport, PhysicsError> {
    Ok(BenchmarkReport {
        steps,
        props,
        sdf: run_bin(ColliderMode::Sdf, library, steps, props)?,
        boxes: run_bin(ColliderMode::BoundingBox, library, steps, props)?,
    })
}

fn run_bin(mode: ColliderMode, library: &PropLibrary, steps: u32, props: usize) -> Result<BinReport, PhysicsError> {
    let mut arena = PileArena::new(mode, 0.0, None)?;
    arena.set_target(props);

    let settled_from = steps.saturating_sub(SETTLED_STEPS);
    let mut total_milliseconds = 0.0;
    let mut settled_milliseconds = 0.0;
    for step in 0..steps {
        let stopwatch = Stopwatch::start();
        arena.update(PHYSICS.fixed_step, library, None)?;
        let elapsed = stopwatch.elapsed_milliseconds();
        total_milliseconds += elapsed;
        if step >= settled_from {
            settled_milliseconds += elapsed;
        }
    }
    Ok(BinReport {
        milliseconds_per_step: total_milliseconds / f64::from(steps.max(1)),
        settled_milliseconds_per_step: settled_milliseconds / f64::from((steps - settled_from).max(1)),
        hash: arena.position_hash(),
        snapshot: arena.snapshot(),
    })
}

impl fmt::Display for BenchmarkReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let SdfCollisionStatistics { pairs, contacts, iterations, distance_queries } = self.sdf.snapshot.statistics;
        write!(
            formatter,
            "SDF benchmark: {} props, {} steps | SDF {:.3} ms/step ({:.3} settled), hash {:016x}, {} left the bin | \
             boxes {:.3} ms/step ({:.3} settled), hash {:016x}, {} left the bin | last step: {pairs} pairs, \
             {contacts} contacts, {iterations} iterations, {distance_queries} distance queries",
            self.props,
            self.steps,
            self.sdf.milliseconds_per_step,
            self.sdf.settled_milliseconds_per_step,
            self.sdf.hash,
            self.sdf.snapshot.escaped,
            self.boxes.milliseconds_per_step,
            self.boxes.settled_milliseconds_per_step,
            self.boxes.hash,
            self.boxes.snapshot.escaped,
        )
    }
}
