// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The FORM DESTRUCTION example (`examples/form-destruction`), ported from its Node tests and browser capture: seeded
//! structures and reset, persistent debris that lands and settles, bounded impact markers, support-released chunks
//! caught by surviving colliders, and the demo driven by real pointer and key input through cumulative damage, a
//! breach, the debug views, a miss, a reset, a broad undercut, and an orbit that applies no damage.
//!
//! The demo test needs a GPU adapter (llvmpipe or SwiftShader is enough); without one it prints a note and returns.
//! Set `JOY_RENDER_CAPTURE_DIR` to keep PNG captures. Native only.
#![cfg(not(target_arch = "wasm32"))]

#[path = "../../examples/form-destruction/src/mod.rs"]
mod example;

use example::config::destruction_config::{
    DESTRUCTION,
    IMPACT_DEBUG,
    SUPPORT,
    StructureView,
    VIEW,
};
use example::destruction_debris::DestructionDebris;
use example::destruction_demo::{
    FormDestructionDemo,
    input_map,
};
use example::impact_debug::ImpactDebugDraw;
use example::structure::generate_structure;
use glam::{
    Vec2,
    Vec3,
};
use joy_engine::physics::{
    ChunkStructure,
    DestructibleChunkDefinition,
    RadialDamage,
};
use joy_engine::rendering::{
    Camera,
    OrbitCamera,
    RenderScene,
    project_to_viewport,
};
use joy_engine::{
    HeadlessOptions,
    HeadlessShell,
};

const STEP: f32 = 1.0 / 60.0;

#[test]
fn identical_seeds_reproduce_the_structure_and_reset_restores_collision() {
    assert_eq!(generate_structure(42), generate_structure(42));
    assert_ne!(generate_structure(42), generate_structure(43));
    assert_eq!(generate_structure(DESTRUCTION.seed).len(), 126);
    let mut structure = ChunkStructure::new(&generate_structure(42), None).unwrap();
    for _ in 0..6 {
        structure.queue_damage(RadialDamage { center: [0.0, 1.0, 0.0], radius: 1.3, damage: 1.0 }).unwrap();
        structure.commit();
    }
    let query = [0.34, 0.82, 0.34];
    assert!(structure.can_occupy([0.0, 0.9, 0.0], query).unwrap(), "six hits open the wall");
    structure.reset();
    assert!(!structure.can_occupy([0.0, 0.9, 0.0], query).unwrap(), "reset restores collision");
}

#[test]
fn released_chunks_land_persist_and_release_on_reset() {
    let mut scene = RenderScene::new();
    let mut debris = DestructionDebris::new(&mut scene).unwrap();
    let chunks = generate_structure(DESTRUCTION.seed);
    let chunk = DestructibleChunkDefinition { center: [0.0, 4.0, 0.0], ..chunks[0].clone() };
    debris.spawn(&mut scene, [&chunk], true).unwrap();
    for _ in 0..60 {
        debris.update(&mut scene, STEP).unwrap();
        assert!(debris.snapshot().pieces.iter().all(|piece| piece.position.y > 0.2), "debris stays above the ground");
    }
    assert!(debris.snapshot().pieces[0].position.y < 1.0, "a chunk released four meters up lands within a second");
    for _ in 0..60 * 20 {
        debris.update(&mut scene, STEP).unwrap();
    }
    assert_eq!(debris.count(), 1, "debris persists beyond the former lifetime");
    assert!(debris.snapshot().pieces[0].velocity.length() < 0.15, "ground friction stops debris sliding");

    debris.spawn(&mut scene, &chunks, true).unwrap();
    debris.spawn(&mut scene, &chunks, true).unwrap();
    assert_eq!(debris.count(), chunks.len(), "every chunk persists; duplicate releases allocate nothing");
    assert_eq!(scene.len(), chunks.len() + 1);
    debris.reset(&mut scene);
    assert!(debris.snapshot().pieces.is_empty());
    assert_eq!(scene.len(), 1, "reset keeps the ground and releases every debris mesh");
    debris.spawn(&mut scene, [&chunk], true).unwrap();
    assert_eq!(debris.count(), 1, "a reset lets the same chunk release again");
    debris.dispose(&mut scene);
    assert!(scene.is_empty());
}

#[test]
fn impact_markers_stay_bounded_and_release_their_meshes() {
    let mut scene = RenderScene::new();
    let mut markers = ImpactDebugDraw::new();
    for index in 0..IMPACT_DEBUG.maximum_markers + 5 {
        markers.add(&mut scene, [index as f64, 2.0, 0.0], Vec3::NEG_Z, 1.0).unwrap();
    }
    assert_eq!(scene.len(), IMPACT_DEBUG.maximum_markers);
    markers.set_enabled(&mut scene, false);
    markers.add(&mut scene, [0.0, 2.0, 0.0], Vec3::NEG_Z, 1.0).unwrap();
    assert!(scene.is_empty(), "disabled markers release existing meshes and allocate none");

    markers.set_enabled(&mut scene, true);
    markers.add(&mut scene, [0.0, 2.0, 0.0], Vec3::NEG_Z, 1.0).unwrap();
    assert_eq!(scene.len(), 1);
    markers.update(&mut scene, IMPACT_DEBUG.lifetime_seconds);
    assert!(scene.is_empty(), "expired markers release their meshes");
    markers.add(&mut scene, [0.0, 2.0, 0.0], Vec3::NEG_Z, 1.0).unwrap();
    markers.clear(&mut scene);
    assert!(scene.is_empty() && markers.enabled());
}

#[test]
fn support_released_chunks_rest_on_surviving_structure_and_fractures_escape() {
    let mut scene = RenderScene::new();
    let mut debris = DestructionDebris::new(&mut scene).unwrap();
    let foot = DestructibleChunkDefinition {
        id: "foot".to_owned(),
        center: [0.0, 0.5, 0.0],
        size: [1.0, 1.0, 1.0],
        material: "masonry".to_owned(),
        hit_points: 6.0,
    };
    let upper = DestructibleChunkDefinition { id: "upper".to_owned(), center: [0.0, 4.0, 0.0], ..foot.clone() };
    debris.synchronize_structure([&foot]).unwrap();
    debris.spawn(&mut scene, [&upper], false).unwrap();
    assert_eq!(debris.snapshot().pieces[0].velocity, Vec3::ZERO, "support failures fall without an impulse");
    for _ in 0..180 {
        debris.update(&mut scene, STEP).unwrap();
    }
    let height = debris.snapshot().pieces[0].position.y;
    assert!(height > 1.4 && height < 1.6, "the surviving wall catches the falling chunk at {height}");
    debris.synchronize_structure([]).unwrap();
    for _ in 0..180 {
        debris.update(&mut scene, STEP).unwrap();
    }
    assert!(debris.snapshot().pieces[0].position.y < 0.6, "removing the collider lets the chunk reach the ground");
    assert_eq!(debris.snapshot().static_chunks, 0);
    debris.reset(&mut scene);
    debris.synchronize_structure([&foot]).unwrap();
    assert_eq!((debris.snapshot().static_chunks, debris.count()), (1, 0));

    let mut structure = ChunkStructure::new(&generate_structure(DESTRUCTION.seed), Some(SUPPORT.options())).unwrap();
    for _ in 0..6 {
        structure.queue_damage(RadialDamage { center: [-2.0, 2.5, 0.275], radius: 1.15, damage: 1.0 }).unwrap();
    }
    let changes = structure.commit();
    let detached: Vec<&str> = changes.detached.iter().map(|chunk| chunk.id.as_str()).collect();
    debris.synchronize_structure(structure.active_chunks()).unwrap();
    debris
        .spawn(&mut scene, changes.removed.iter().filter(|chunk| !detached.contains(&chunk.id.as_str())), true)
        .unwrap();
    debris.spawn(&mut scene, &changes.detached, false).unwrap();
    for _ in 0..180 {
        debris.update(&mut scene, STEP).unwrap();
    }
    assert!(
        debris.snapshot().pieces.iter().any(|piece| piece.position.z > 0.5 && piece.position.y < 1.0),
        "fractured chunks escape the wall instead of wedging between intact neighbors"
    );
}

/// Drives the demo through the headless shell with real pointer and key input.
struct Driver {
    shell: HeadlessShell,
    demo: FormDestructionDemo,
}

impl Driver {
    fn frames(&mut self, count: u32) {
        self.shell.run_frames(&mut self.demo, count).expect("demo frames");
    }

    fn click(&mut self, point: Vec2) {
        self.shell.input().handle_pointer_move(point);
        self.shell.input().handle_mouse_button(0, true);
        self.frames(1);
        self.shell.input().handle_mouse_button(0, false);
        self.frames(1);
    }

    fn drag(&mut self, from: Vec2, to: Vec2) {
        self.shell.input().handle_pointer_move(from);
        self.shell.input().handle_mouse_button(0, true);
        self.frames(1);
        for step in 1..=8 {
            self.shell.input().handle_pointer_move(from.lerp(to, step as f32 / 8.0));
            self.frames(1);
        }
        self.shell.input().handle_mouse_button(0, false);
        self.frames(1);
    }

    fn key(&mut self, code: &str) {
        self.shell.input().handle_key(code, true);
        self.frames(1);
        self.shell.input().handle_key(code, false);
        self.frames(1);
    }

    fn capture(&mut self, name: &str) -> Vec<u8> {
        if let Ok(directory) = std::env::var("JOY_RENDER_CAPTURE_DIR") {
            std::fs::create_dir_all(&directory).expect("capture directory");
            self.shell.save_png(&std::path::Path::new(&directory).join(format!("{name}.png"))).expect("png written");
        }
        self.shell.capture().expect("readback")
    }

    fn remaining(&self, chunk_id: &str) -> f64 {
        let damage = self.demo.snapshot().damage;
        damage.iter().find(|state| state.chunk_id == chunk_id).map_or(f64::NAN, |state| state.remaining_hit_points)
    }
}

/// Where a world point appears in the headless viewport from the demo's starting camera.
fn screen_point(world: Vec3) -> Vec2 {
    let camera = OrbitCamera {
        target: VIEW.target,
        distance: VIEW.distance,
        radius: VIEW.radius,
        yaw: VIEW.yaw,
        pitch: VIEW.pitch,
        near_clip: Some(VIEW.near_clip),
        far_clip: Some(VIEW.far_clip),
        ..OrbitCamera::default()
    };
    let size = Vec2::new(640.0, 400.0);
    project_to_viewport(world, camera.camera_frame(size.x / size.y).view_projection(), size).expect("on screen")
}

#[test]
fn the_demo_breaks_supports_and_keeps_debris_through_real_input() {
    let options = HeadlessOptions { width: 640, height: 400, input: input_map(), ..Default::default() };
    let shell = match HeadlessShell::new(options) {
        Ok(shell) => shell,
        Err(error) => {
            eprintln!("skipping GPU test: {error}");
            return;
        }
    };
    let mut driver = Driver { shell, demo: FormDestructionDemo::new().unwrap() };
    driver.shell.start(&mut driver.demo).unwrap();
    driver.frames(2);
    let intact = driver.capture("example-form-destruction-integrity");
    assert_eq!(driver.demo.snapshot().active_chunks, 126);
    assert_eq!(driver.demo.snapshot().debris.static_chunks, 126, "surviving chunks have static colliders");

    // One click weakens the chunk under the pointer without breaking it.
    let center = screen_point(Vec3::new(0.0, 2.5, 0.275));
    driver.click(center);
    let snapshot = driver.demo.snapshot();
    let target = snapshot.last_impact.expect("the click hits the wall").chunk_id;
    assert_eq!(target, "front:6:2");
    assert_eq!(driver.remaining(&target), 5.0);
    assert_eq!(snapshot.notice, "Wall resisted · 5 more impacts here to break");
    assert_eq!(snapshot.impact_debug, (true, 1));
    let weakened = driver.capture("example-form-destruction-first-impact");
    assert!(intact != weakened, "integrity colors and the marker change the frame");

    // Six hits breach intact masonry; the debris settles on the ground and stays.
    for _ in 0..5 {
        driver.click(center);
    }
    let breached = driver.demo.snapshot();
    assert!(breached.active_chunks < 126 && breached.debris.pieces.len() >= 126 - breached.active_chunks);
    assert!(breached.notice.contains("broken"), "{}", breached.notice);
    driver.frames(150);
    let settled = driver.demo.snapshot();
    let top = settled.debris.ground_top as f32;
    assert!(settled.debris.pieces.iter().all(|piece| piece.position.y > top), "debris rests on the ground");
    assert!(settled.debris.pieces.iter().all(|piece| piece.velocity.length() < 1.0), "debris settles");
    assert_eq!(settled.impact_debug.1, 6, "each impact keeps a marker for six seconds");
    driver.capture("example-form-destruction-breach");

    driver.demo.set_view(StructureView::Support).unwrap();
    driver.frames(1);
    driver.capture("example-form-destruction-support");
    driver.demo.set_view(StructureView::Material).unwrap();
    driver.frames(1);
    driver.capture("example-form-destruction-material");

    // Empty space changes nothing; R restores the structure and clears debris.
    driver.click(Vec2::new(8.0, 8.0));
    assert_eq!(driver.demo.snapshot().notice, "No structure at this point");
    assert!(driver.demo.snapshot().last_impact.is_none());
    driver.key("KeyR");
    let reset = driver.demo.snapshot();
    assert_eq!((reset.active_chunks, reset.debris.pieces.len()), (126, 0));
    assert_eq!(reset.notice, "Structure reset");

    // A broad undercut breaks the lower middle and healthy chunks above lose support and fall.
    driver.demo.set_view(StructureView::Integrity).unwrap();
    driver.demo.set_damage(2.0, 3.0);
    let base = screen_point(Vec3::new(0.0, 0.6, 0.275));
    driver.click(base);
    driver.click(base);
    let undercut = driver.demo.snapshot();
    assert!(!undercut.detached_chunks.is_empty(), "unsupported chunks detach with health remaining");
    let detached = &undercut.detached_chunks[0];
    assert!(undercut.damage.iter().any(|state| state.chunk_id == *detached && state.remaining_hit_points > 0.0));
    driver.frames(90);
    driver.capture("example-form-destruction-undercut");

    // Orbiting rotates the view without striking.
    let damage = driver.demo.snapshot().damage;
    driver.drag(Vec2::new(500.0, 300.0), Vec2::new(420.0, 280.0));
    let orbited = driver.demo.snapshot();
    assert_eq!(orbited.damage, damage, "drags never apply damage");
    driver.capture("example-form-destruction-orbit");
}
