// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Terrain collision: heightfields split cells like the terrain lattice, primitive and SDF bodies rest on terrain
//! colliders where gameplay sampling says the ground is, and craters rebuild only the colliders they reach.

use joy_core::PrimitiveShape;
use joy_core::math::{
    Vec3,
    deterministic,
};
use joy_physics::{
    BodyOptions,
    HeightfieldOptions,
    PhysicsError,
    PhysicsWorld,
    PhysicsWorldOptions,
    SdfPrimitive,
    SdfShape,
    TerrainColliderOptions,
    TerrainColliders,
};
use joy_terrain::{
    MemoryTileLoader,
    TerrainCrater,
    TerrainInterest,
    TerrainLayout,
    TerrainStreamer,
    TerrainStreamerOptions,
    TerrainSurface,
    TerrainTile,
    build_terrain_pyramid,
};

/// Advance in 60 Hz frames for a number of seconds.
fn simulate(world: &mut PhysicsWorld, seconds: f32) {
    for _ in 0..(seconds * 60.0).round() as usize {
        world.step(1.0 / 60.0).unwrap();
    }
}

/// A gentle 64 x 64 meter landscape: two levels of 16-cell tiles at one meter per sample, centered on the origin.
fn hills() -> (TerrainLayout, Vec<TerrainTile>) {
    let layout = TerrainLayout {
        tile_cells: 16,
        levels: 2,
        root_tiles: [2, 2],
        origin: [-32.0, -32.0],
        spacing: [1.0, 1.0],
        height_offset: -8.0,
        height_scale: 32.0,
    };
    let mut heights = Vec::new();
    for z in 0..=64 {
        for x in 0..=64 {
            let (x, z) = (f64::from(x), f64::from(z));
            heights.push((1.5 * deterministic::sin(x * 0.15)) + (0.8 * deterministic::cos(z * 0.21)) + (0.02 * x));
        }
    }
    let tiles = build_terrain_pyramid(&layout, &heights).expect("the hills are valid");
    (layout, tiles)
}

#[test]
fn heightfields_split_cells_on_the_terrain_diagonal() {
    let mut world = PhysicsWorld::new(PhysicsWorldOptions::default()).unwrap();
    let corners = [0.0, 4.0, 1.0, 2.0];
    world
        .create_heightfield_body(&HeightfieldOptions {
            origin: Vec3::new(10.0, 0.0, 20.0),
            spacing: [2.0, 2.0],
            columns: 2,
            rows: 2,
            heights: corners.to_vec(),
            friction: 0.5,
            restitution: 0.0,
        })
        .unwrap();

    // The diagonal runs from the cell's -X, -Z corner to its +X, +Z corner: the +X side interpolates corners a, b,
    // and d; the +Z side a, c, and d.
    let [a, b, c, d] = corners;
    for (fx, fz) in [(0.75_f32, 0.25_f32), (0.25, 0.75), (0.5, 0.1), (0.1, 0.9)] {
        let expected = if fx >= fz { a + (b - a) * fx + (d - b) * fz } else { a + (d - c) * fx + (c - a) * fz };
        let (x, z) = (10.0 + (fx * 2.0), 20.0 + (fz * 2.0));
        let hit = world.raycast(Vec3::new(x, 10.0, z), Vec3::new(x, -10.0, z)).expect("the ray hits the cell");
        assert!((hit.point.y - expected).abs() < 1e-4, "({fx}, {fz}): {} != {expected}", hit.point.y);
    }

    let invalid = HeightfieldOptions {
        origin: Vec3::ZERO,
        spacing: [1.0, 1.0],
        columns: 2,
        rows: 2,
        heights: vec![0.0, 0.0, f32::NAN, 0.0],
        friction: 0.5,
        restitution: 0.0,
    };
    assert_eq!(world.create_heightfield_body(&invalid), Err(PhysicsError::InvalidHeightfield));
}

#[test]
fn primitive_and_sdf_bodies_rest_where_gameplay_samples_the_ground() {
    let run = || {
        let (layout, tiles) = hills();
        let surface = TerrainSurface::from_tiles(layout, tiles, Default::default()).unwrap();
        let mut world = PhysicsWorld::new(PhysicsWorldOptions::default()).unwrap();
        let mut colliders = TerrainColliders::new(TerrainColliderOptions {
            radius: 12.0,
            ..Default::default()
        })
        .unwrap();
        let report = colliders.sync(&mut world, &surface, &[Vec3::new(4.0, 0.0, -3.0)]).unwrap();
        assert!(report.built >= 1 && report.colliders == report.built);

        let ball = world
            .create_body(BodyOptions {
                mass: 1.0,
                radius: 0.4,
                position: Vec3::new(1.0, 6.0, -2.0),
                friction: 1.0,
                angular_damping: 0.9,
                ..BodyOptions::new(PrimitiveShape::Sphere)
            })
            .unwrap();
        let ring = SdfShape::primitive(SdfPrimitive::Torus {
            major_radius: 0.6,
            minor_radius: 0.2,
        })
        .unwrap();
        let prop = world
            .create_sdf_body(
                &ring,
                BodyOptions {
                    mass: 1.0,
                    position: Vec3::new(6.0, 6.0, -4.0),
                    linear_damping: 0.3,
                    angular_damping: 0.9,
                    ..BodyOptions::default()
                },
            )
            .unwrap();
        simulate(&mut world, 5.0);
        let statistics = world.sdf_collision_statistics();
        assert!(statistics.pairs >= 1 && statistics.contacts >= 1, "the ring touches terrain: {statistics:?}");
        let ball_pose = world.body_transform(ball).unwrap();
        let prop_pose = world.body_transform(prop).unwrap();
        (surface, ball_pose, prop_pose, world.body_velocity(prop).unwrap())
    };
    let (surface, ball, prop, prop_velocity) = run();

    // The ball settles one radius above the sampled ground along the surface normal (a little more on a slope).
    let ground = surface.height_at(f64::from(ball.position.x), f64::from(ball.position.z)).unwrap() as f32;
    let clearance = ball.position.y - ground;
    assert!(clearance > 0.38 && clearance < 0.5, "the ball rests on the terrain: {clearance}");

    // The SDF ring rests on the heightfield instead of falling through it.
    let ground = surface.height_at(f64::from(prop.position.x), f64::from(prop.position.z)).unwrap() as f32;
    let clearance = prop.position.y - ground;
    assert!(clearance > -0.05 && clearance < 0.6, "the SDF prop rests on the terrain: {clearance}");
    assert!(prop_velocity.length() < 0.2, "the SDF prop settles: {prop_velocity:?}");

    // Colliders join the world in tile order, so identical worlds stay identical.
    let (_, ball_again, prop_again, _) = run();
    assert_eq!((ball, prop), (ball_again, prop_again));
}

#[test]
fn craters_rebuild_only_the_colliders_they_reach() {
    let (layout, tiles) = hills();
    let loader = MemoryTileLoader::new(tiles, 0);
    let mut streamer = TerrainStreamer::new(layout, Box::new(loader), TerrainStreamerOptions::default()).unwrap();
    let anchor = Vec3::new(-6.0, 0.0, 5.0);
    let region = TerrainInterest::Collision {
        center: [f64::from(anchor.x), f64::from(anchor.z)],
        radius: 20.0,
    };
    for _ in 0..4 {
        streamer.update(&[region]).unwrap();
    }
    let mut world = PhysicsWorld::new(PhysicsWorldOptions::default()).unwrap();
    let mut colliders = TerrainColliders::new(TerrainColliderOptions {
        radius: 20.0,
        ..Default::default()
    })
    .unwrap();
    let surface = streamer.surface().unwrap().clone();
    let first = colliders.sync(&mut world, &surface, &[anchor]).unwrap();
    assert_eq!((first.colliders, first.built), (9, 9), "the region spans nine level-0 tiles");
    assert_eq!(colliders.sync(&mut world, &surface, &[anchor]).unwrap().built, 0, "unchanged tiles stay");

    // A crate resting at the crater's center sinks into the bowl once the colliders rebuild.
    let crate_body = world
        .create_body(BodyOptions {
            mass: 1.0,
            half_extents: Vec3::splat(0.3),
            position: Vec3::new(-10.0, 4.0, 10.0),
            friction: 1.0,
            angular_damping: 0.9,
            ..BodyOptions::new(PrimitiveShape::Box)
        })
        .unwrap();
    simulate(&mut world, 3.0);
    let before = world.body_transform(crate_body).unwrap().position.y;
    streamer
        .add_crater(TerrainCrater {
            center: [-10.0, 10.0],
            radius: 3.0,
            depth: 1.2,
            rim_height: 0.2,
        })
        .unwrap();
    streamer.update(&[region]).unwrap();
    let edited = colliders.sync(&mut world, streamer.surface().unwrap(), &[anchor]).unwrap();
    assert_eq!(edited.built, 1, "only the tile under the crater rebuilds");
    simulate(&mut world, 3.0);

    // The crate drops into the bowl and rests on the edited surface gameplay samples there.
    let after = world.body_transform(crate_body).unwrap().position;
    let offset = (after.x + 10.0).hypot(after.z - 10.0);
    let ground = streamer.surface().unwrap().height_at(f64::from(after.x), f64::from(after.z)).unwrap() as f32;
    assert!(offset < 1.5, "the crate stays in the bowl ({offset} from its center)");
    assert!(before - after.y > 0.6, "the crate settles into the crater: {before} -> {}", after.y);
    assert!((after.y - ground - 0.3).abs() < 0.15, "the crate rests on the cratered surface: {}", after.y - ground);
}
