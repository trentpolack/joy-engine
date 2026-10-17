// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The SDF COLLISIONS example (`examples/sdf-collisions`): its FORM props bake into SDF colliders that fit their
//! meshes, the headless benchmark fills both bins identically on every run with the SDF pile kept inside its bin, and
//! the demo drops, pauses, adds, and resets props through real key input.
//!
//! The demo test needs a GPU adapter (llvmpipe or SwiftShader is enough); without one it prints a note and returns.
//! Set `JOY_RENDER_CAPTURE_DIR` to keep a PNG capture. Native only.
#![cfg(not(target_arch = "wasm32"))]

#[path = "../../examples/sdf-collisions/src/mod.rs"]
mod example;

use example::benchmark::run_benchmark;
use example::config::sdf_collisions_config::SPAWN;
use example::prop_library::{PROPS, PropLibrary};
use example::sdf_collisions_demo::{SdfCollisionsDemo, input_map};
use joy_engine::runtime::key;
use joy_engine::{HeadlessOptions, HeadlessShell};

#[test]
fn props_bake_into_colliders_that_fit_their_meshes() {
    let library = PropLibrary::load().unwrap();
    assert_eq!(library.len(), PROPS.len());
    for prop in library.props() {
        // Each collider fits its render mesh's bounds and carries mass and seeds for stochastic sampling.
        let mesh_bounds = prop.asset.bounds();
        let (minimum, maximum) = prop.shape.local_bounds();
        assert!(
            (minimum - mesh_bounds.min).abs().max_element() < 0.02,
            "{}: {minimum} vs {}",
            prop.name,
            mesh_bounds.min
        );
        assert!(
            (maximum - mesh_bounds.max).abs().max_element() < 0.02,
            "{}: {maximum} vs {}",
            prop.name,
            mesh_bounds.max
        );
        assert!(prop.mass > 1.0 && prop.shape.seed_count() > 0, "{}", prop.name);
        assert!(prop.box_half_extents.min_element() > 0.0);
    }
}

#[test]
fn benchmark_piles_repeat_exactly_and_stay_in_their_bins() {
    let library = PropLibrary::load().unwrap();
    let first = run_benchmark(&library, 240, 8).unwrap();
    let second = run_benchmark(&library, 240, 8).unwrap();
    assert_eq!(first.sdf.hash, second.sdf.hash, "SDF contact sampling is seeded, so piles repeat bit for bit");
    assert_eq!(first.boxes.hash, second.boxes.hash);
    assert_ne!(first.sdf.hash, first.boxes.hash, "the bins differ only in collision, and that shows");
    assert_eq!(first.sdf.snapshot.bodies, 8);
    assert_eq!(first.sdf.snapshot.escaped, 0, "no SDF prop falls through the floor or walls");
    assert!(first.sdf.snapshot.statistics.contacts > 0, "{:?}", first.sdf.snapshot.statistics);
}

#[test]
fn the_demo_drops_pauses_adds_and_resets_props_through_key_input() {
    let options = HeadlessOptions { width: 640, height: 400, input: input_map(), ..Default::default() };
    let mut shell = match HeadlessShell::new(options) {
        Ok(shell) => shell,
        Err(error) => {
            eprintln!("skipping GPU test: {error}");
            return;
        }
    };
    let mut demo = SdfCollisionsDemo::new().unwrap();
    shell.start(&mut demo).unwrap();
    // Both bins receive the same drops on the same schedule (one every 0.3 seconds at 60 frames per second).
    shell.run_frames(&mut demo, 60).unwrap();
    let dropped = demo.snapshot();
    assert_eq!(dropped.sdf.bodies, dropped.boxes.bodies);
    assert!(dropped.sdf.bodies >= 3, "{dropped:?}");
    if let Ok(directory) = std::env::var("JOY_RENDER_CAPTURE_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        shell.save_png(&std::path::Path::new(&directory).join("example-sdf-collisions.png")).unwrap();
    }

    // P freezes both piles; Space raises both targets; R empties both bins and restores the initial pile size.
    press(&mut shell, &mut demo, key::KEY_P);
    let paused = demo.snapshot();
    shell.run_frames(&mut demo, 30).unwrap();
    assert!(paused.paused && demo.snapshot().sdf.bodies == paused.sdf.bodies);
    press(&mut shell, &mut demo, key::KEY_P);
    press(&mut shell, &mut demo, key::SPACE);
    assert_eq!(demo.snapshot().sdf.target, SPAWN.initial_count + SPAWN.burst);
    assert_eq!(demo.snapshot().boxes.target, SPAWN.initial_count + SPAWN.burst);
    press(&mut shell, &mut demo, key::KEY_R);
    let reset = demo.snapshot();
    assert_eq!((reset.sdf.bodies, reset.boxes.bodies), (0, 0));
    assert_eq!(reset.sdf.target, SPAWN.initial_count);
}

/// Press and release a key over two frames.
fn press(shell: &mut HeadlessShell, demo: &mut SdfCollisionsDemo, code: &str) {
    shell.input().handle_key(code, true);
    shell.run_frames(demo, 1).unwrap();
    shell.input().handle_key(code, false);
    shell.run_frames(demo, 1).unwrap();
}
