// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The shell's fixed simulation tick: a frame runs every tick it owes between `input` and `simulation`, ticks are
//! numbered without gaps, and presentation sees the fraction toward the next one. Runs through `HeadlessShell`, which
//! shares the windowed shell's ticker.
//!
//! Needs a GPU adapter (llvmpipe or SwiftShader is enough); without one the test prints a note and returns.
#![cfg(not(target_arch = "wasm32"))]

use joy_engine::rendering::{
    FrameInputs,
    OrbitCamera,
    RenderScene,
};
use joy_engine::runtime::{
    App,
    AppResult,
    FixedTick,
    Frame,
    RenderTarget,
};
use joy_engine::{
    HeadlessOptions,
    HeadlessShell,
};

/// Records the phase order and every tick it receives.
#[derive(Default)]
struct TickRecorder {
    scene: RenderScene,
    camera: OrbitCamera,
    phases: Vec<&'static str>,
    ticks: Vec<FixedTick>,
    interpolations: Vec<f32>,
}

impl App for TickRecorder {
    fn input(&mut self, _frame: &mut Frame<'_>) -> AppResult {
        self.phases.push("input");
        Ok(())
    }

    fn fixed_update(&mut self, _frame: &mut Frame<'_>, tick: FixedTick) -> AppResult {
        self.phases.push("fixed");
        self.ticks.push(tick);
        Ok(())
    }

    fn simulation(&mut self, frame: &mut Frame<'_>) -> AppResult {
        self.phases.push("simulation");
        self.interpolations.push(frame.time.fixed_interpolation);
        Ok(())
    }

    fn render(&mut self, _frame: &mut Frame<'_>, target: &mut RenderTarget<'_>) -> AppResult {
        Ok(target.draw(&self.scene, &self.camera, &FrameInputs::default())?)
    }
}

#[test]
fn frames_run_the_ticks_they_owe_before_simulation() {
    // A 40 Hz display driving a 60 Hz simulation owes one and a half ticks per frame.
    let Some(mut shell) = HeadlessShell::new_or_skip(HeadlessOptions {
        width: 64,
        height: 64,
        frame_seconds: 1.0 / 40.0,
        ..Default::default()
    }) else {
        return;
    };
    let mut app = TickRecorder::default();
    for _ in 0..5 {
        shell.step(&mut app).unwrap();
    }

    // The first frame has no elapsed time; the next four accumulate 0.1 seconds, which is six whole ticks.
    assert_eq!(app.ticks.len(), 6);
    assert!(app.ticks.iter().enumerate().all(|(index, tick)| tick.index == index as u64));
    assert!(app.ticks.iter().all(|tick| (tick.step_seconds - (1.0 / 60.0)).abs() < 1.0e-12));
    assert_eq!(
        app.phases[..8],
        ["input", "simulation", "input", "fixed", "simulation", "input", "fixed", "fixed"],
        "ticks run after input and before simulation"
    );
    assert!(app.interpolations.iter().all(|fraction| (0.0..1.0).contains(fraction)));
    assert!(
        (app.interpolations[1] - 0.5).abs() < 1.0e-4,
        "a frame and a half in, presentation is halfway to the next tick"
    );
}
