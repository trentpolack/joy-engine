// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The TERRAIN example (`examples/terrain`): Dragon Pit streams into synthesized detail near the ground, C blasts a
//! crater that lowers the gameplay surface, P drops SDF crates and spheres that come to rest on the terrain colliders,
//! and a package written to disk streams through the platform fetcher.
//!
//! The lab test needs a GPU adapter (llvmpipe or lavapipe is enough); without one it prints a note and returns. Set
//! `JOY_CAPTURE_DIR` to keep PNG captures. Native only.
#![cfg(not(target_arch = "wasm32"))]

#[path = "../../examples/terrain/src/mod.rs"]
mod example;

use example::terrain_config::TERRAIN_CONFIG;
use example::terrain_lab::{
    LabSnapshot,
    TerrainLab,
    input_map,
    renderer_options,
};
use example::terrain_world::{
    PropKind,
    TerrainWorld,
    terrain_view,
};
use joy_engine::math::Vec3;
use joy_engine::runtime::key;
use joy_engine::terrain::{
    PACKAGE_MANIFEST_FILE,
    TerrainPackage,
    build_pyramid_from_asset,
    parse_terrain,
    write_terrain_package,
};
use joy_engine::{
    HeadlessOptions,
    HeadlessShell,
};

/// Settled crates move slower than this (world units per second) even when a rolling sphere nudges them.
const NUDGED_SPEED: f32 = 1.0;

#[test]
fn the_lab_streams_detail_blasts_craters_and_lands_props() {
    let Some(mut shell) = HeadlessShell::new_or_skip(HeadlessOptions {
        width: 320,
        height: 200,
        input: input_map(),
        renderer: renderer_options(),
        ..Default::default()
    }) else {
        return;
    };
    let mut lab = TerrainLab::new().unwrap();
    shell.start(&mut lab).unwrap();
    shell.run_frames(&mut lab, 30).unwrap();
    assert!(world(&lab).ready, "the roots arrive within a few frames");
    shell.save_capture("example-terrain-overview.png");

    // A grazing view near the focus refines past the authored lattice into synthesized detail.
    press(&mut shell, &mut lab, key::KEY_G);
    shell.run_frames(&mut lab, 60).unwrap();
    let terrain = lab.snapshot().terrain.expect("the terrain is drawn");
    let stats = shell.renderer().terrain_stats(terrain).expect("terrain stats").clone();
    assert!(stats.finest_level < 0, "the grazing view draws synthesized detail: {stats:?}");
    shell.save_capture("example-terrain-ground.png");

    // C blasts a crater under the pointer, here pointed at the ground under the focus, and lowers the gameplay surface.
    let LabSnapshot {
        focus,
        camera,
        ..
    } = lab.snapshot();
    let before = world(&lab).focus_ground.expect("the collision level is resident under the focus") as f32;
    let pointer = shell.renderer().project(&camera, Vec3::new(focus.x, before, focus.z)).expect("the focus is in view");
    shell.input().handle_pointer_move(pointer);
    press(&mut shell, &mut lab, key::KEY_C);
    shell.run_frames(&mut lab, 2).unwrap();
    let snapshot = world(&lab);
    assert_eq!(snapshot.craters.len(), 1);
    let crater = snapshot.craters[0];
    let offset = Vec3::new(crater.center[0] as f32 - focus.x, 0.0, crater.center[1] as f32 - focus.z).length();
    assert!(offset < 1.0, "the crater lands under the pointer ({offset} from the focus)");
    let after = snapshot.focus_ground.expect("the focus stays resident") as f32;
    assert!(before - after > (crater.depth as f32) * 0.8, "the crater lowered the ground: {before} -> {after}");
    shell.save_capture("example-terrain-crater.png");

    // P drops a burst of props that settle on the terrain instead of falling through it.
    press(&mut shell, &mut lab, key::KEY_P);
    shell.run_frames(&mut lab, 240).unwrap();
    let snapshot = world(&lab);
    assert_eq!(snapshot.props.len(), TERRAIN_CONFIG.props.burst);
    assert!(snapshot.colliders > 0);
    shell.save_capture("example-terrain-props.png");
    // No prop falls through the terrain or is flung off it; SDF crates settle (spheres may still roll in the bowl and
    // nudge them).
    for prop in &snapshot.props {
        let ground = prop.ground.expect("collision tiles stay resident under props") as f32;
        let clearance = prop.pose.position.y - ground;
        assert!(clearance > -0.5 && clearance < 6.0, "{prop:?} rests on the terrain ({clearance})");
        if prop.kind == PropKind::Crate {
            assert!(prop.at_rest(NUDGED_SPEED), "{prop:?} settles");
        }
    }
}

#[test]
fn a_package_streams_through_the_platform_fetcher() {
    let asset = parse_terrain(
        include_str!("../../examples/terrain/assets/dragon-pit/terrain.json"),
        include_bytes!("../../examples/terrain/assets/dragon-pit/height.bin"),
    )
    .unwrap();
    let (layout, tiles) = build_pyramid_from_asset(&asset, 3).unwrap();
    let folder = std::env::temp_dir().join(format!("joy-terrain-example-package-{}", std::process::id()));
    write_terrain_package(&folder, &layout, &tiles).unwrap();
    let package = TerrainPackage::parse(&std::fs::read_to_string(folder.join(PACKAGE_MANIFEST_FILE)).unwrap()).unwrap();

    // The fetcher reads tiles on its worker thread, so the stream refines over real elapsed updates.
    let mut world = TerrainWorld::package(package, folder.to_string_lossy().into_owned(), false).unwrap();
    let focus = Vec3::new(-700.0, 300.0, -650.0);
    let eye = focus + Vec3::new(30.0, 40.0, 30.0);
    let view = terrain_view(eye, focus, 720.0, 2.0, 0.0);
    let mut ground = None;
    for _ in 0..400 {
        world.update_streaming(view, focus).unwrap();
        ground = world.snapshot(focus).focus_ground;
        if ground.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    std::fs::remove_dir_all(&folder).unwrap();
    let expected = joy_engine::terrain::sample_terrain(&asset, f64::from(focus.x), f64::from(focus.z));
    let ground = ground.expect("the package streamed level 0 under the focus");
    assert!((ground - expected).abs() < 0.05, "streamed ground {ground} matches the asset's {expected}");
}

/// The world snapshot, which exists once the tile source is ready.
fn world(lab: &TerrainLab) -> example::terrain_world::WorldSnapshot {
    let LabSnapshot {
        world,
        ..
    } = lab.snapshot();
    world.expect("the embedded source is ready immediately")
}

/// Press and release a key over two frames.
fn press(shell: &mut HeadlessShell, lab: &mut TerrainLab, code: &str) {
    shell.input().handle_key(code, true);
    shell.run_frames(lab, 1).unwrap();
    shell.input().handle_key(code, false);
    shell.run_frames(lab, 1).unwrap();
}
