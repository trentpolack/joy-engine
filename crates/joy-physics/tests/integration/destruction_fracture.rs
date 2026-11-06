// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Fractured structures end to end: Voronoi pieces fill their solid exactly and repeat for a seed, shared faces bond a
//! fractured wall into a standing structure that loses what an impact undermines, and released shards are rigid debris
//! that land on the surviving structure in the same world, sleep, and freeze into rubble within the pool's budget.

use joy_core::SeededRandom;
use joy_core::math::{
    DVec3,
    Vec3,
};
use joy_physics::{
    ChunkStructure,
    ChunkSupportOptions,
    DebrisOptions,
    DebrisPool,
    DebrisRelease,
    DestructibleChunkDefinition,
    PhysicsWorld,
    PhysicsWorldOptions,
    RadialDamage,
    StructureColliderOptions,
    StructureColliders,
    fracture_chunk,
    fracture_sites,
};

/// A 4 x 3 x 0.5 meter wall standing on the ground at y = 0.
fn wall() -> DestructibleChunkDefinition {
    DestructibleChunkDefinition {
        id: "wall".to_owned(),
        center: DVec3::new(0.0, 1.5, 0.0),
        size: DVec3::new(4.0, 3.0, 0.5),
        material: "masonry".to_owned(),
        hit_points: 1.0,
        shape: None,
    }
}

fn fractured_wall(seed: i64, count: usize) -> Vec<DestructibleChunkDefinition> {
    let wall = wall();
    let sites = fracture_sites(wall.center, wall.size, count, &mut SeededRandom::new(seed));
    fracture_chunk(&wall, &sites).unwrap()
}

#[test]
fn fracture_fills_the_solid_and_repeats_for_a_seed() {
    let pieces = fractured_wall(7, 24);
    assert_eq!(pieces.len(), 24, "every site in the wall owns a piece");

    // The pieces tile the wall: their volumes sum to it and none leaves its bounds.
    let total: f64 = pieces.iter().map(DestructibleChunkDefinition::volume).sum();
    assert!((total - wall().volume()).abs() < 1.0e-9 * wall().volume(), "volume {total}");
    for piece in &pieces {
        let shape = piece.shape.as_ref().expect("fractured pieces carry their shape");
        shape.validate().unwrap();
        let (minimum, maximum) = shape.bounds();
        assert!(
            minimum.cmpge(DVec3::new(-2.0, 0.0, -0.25) - 1.0e-9).all()
                && maximum.cmple(DVec3::new(2.0, 3.0, 0.25) + 1.0e-9).all()
        );
        assert!(piece.id.starts_with("wall/"));
    }
    assert_eq!(pieces, fractured_wall(7, 24), "the same seed fractures identically");
    assert_ne!(pieces, fractured_wall(8, 24));
}

#[test]
fn shared_faces_bond_a_fractured_wall_and_an_impact_undermines_it() {
    let support = ChunkSupportOptions {
        ground_height: 0.0,
        maximum_cantilever_distance: 1.5,
        contact_tolerance: Some(1.0e-6),
    };
    let mut structure = ChunkStructure::new(&fractured_wall(11, 24), Some(support)).unwrap();
    assert!(structure.support_state().iter().all(|state| state.supported), "the intact wall stands");

    // A blast across the wall's lower middle leaves pieces above it without a route to the ground.
    structure
        .queue_damage(RadialDamage {
            center: DVec3::new(0.0, 0.6, 0.0),
            radius: 1.4,
            damage: 10.0,
        })
        .unwrap();
    let change = structure.commit();
    let broken = change.removed.len() - change.detached.len();
    assert!(broken > 0, "the blast breaks the pieces it reaches");
    assert!(!change.detached.is_empty(), "pieces left hanging over the breach lose support");
    assert!(change.detached.iter().all(|piece| piece.center.y > 0.6), "only pieces above the breach lose support");
    assert_eq!(structure.active_chunk_count() + change.removed.len(), 24);
}

#[test]
fn released_shards_land_on_the_surviving_structure_sleep_and_freeze() {
    let mut world = PhysicsWorld::new(PhysicsWorldOptions::default()).unwrap();
    world
        .create_body(joy_physics::BodyOptions {
            half_extents: Vec3::new(10.0, 0.5, 10.0),
            position: Vec3::new(0.0, -0.5, 0.0),
            ..joy_physics::BodyOptions::new(joy_core::PrimitiveShape::Box)
        })
        .unwrap();
    // A 4 x 2 x 4 meter block fractured into 12 pieces, standing on the ground.
    let block = DestructibleChunkDefinition {
        id: "block".to_owned(),
        center: DVec3::new(0.0, 1.0, 0.0),
        size: DVec3::new(4.0, 2.0, 4.0),
        material: "masonry".to_owned(),
        hit_points: 1.0,
        shape: None,
    };
    let sites = fracture_sites(block.center, block.size, 12, &mut SeededRandom::new(3));
    let mut structure = ChunkStructure::new(&fracture_chunk(&block, &sites).unwrap(), None).unwrap();
    let mut colliders = StructureColliders::new(StructureColliderOptions::default());
    colliders.sync(&mut world, structure.active_chunks()).unwrap();
    assert_eq!(colliders.len(), 12);

    // Break the piece nearest the block's top center and drop it from above: it must land on the surviving pieces
    // around and below it, not fall through to the ground.
    let top_center = DVec3::new(0.0, 2.0, 0.0);
    let top = structure
        .active_chunks()
        .min_by(|left, right| left.center.distance(top_center).total_cmp(&right.center.distance(top_center)))
        .cloned()
        .unwrap();
    structure
        .queue_damage(RadialDamage {
            center: top.center,
            radius: 0.01,
            damage: 10.0,
        })
        .unwrap();
    let change = structure.commit();
    colliders.sync(&mut world, structure.active_chunks()).unwrap();
    assert_eq!(colliders.len(), 11);
    let mut lifted = change.removed[0].clone();
    let lift = DVec3::new(0.0, 2.0, 0.0);
    lifted.center += lift;
    lifted.shape = lifted.shape.map(|shape| shape.translated(lift));

    let mut debris = DebrisPool::new(DebrisOptions {
        maximum_moving: 0,
        ..DebrisOptions::default()
    });
    let body = debris.release(&mut world, &lifted, DebrisRelease::default()).unwrap().unwrap();
    assert!(debris.release(&mut world, &lifted, DebrisRelease::default()).unwrap().is_none(), "one piece per chunk");

    let mut frozen = Vec::new();
    for _ in 0..(60 * 6) {
        world.step(1.0 / 60.0).unwrap();
        frozen.extend(debris.settle(&mut world, 1.0 / 60.0).settled);
    }
    let rest = world.body_transform(body).unwrap().position.y;
    assert!(rest > 0.75, "the shard rests on the surviving block (y = {rest}), not on the ground");
    assert_eq!(frozen, vec![lifted.id.clone()], "with no moving budget, a sleeping shard freezes into rubble");
    assert!(debris.piece(&lifted.id).unwrap().settled);
    let above = world.body_transform(body).unwrap().position + Vec3::new(0.0, 5.0, 0.0);
    let hit = world.raycast(above, above - Vec3::new(0.0, 10.0, 0.0)).unwrap();
    assert_eq!(hit.body, body, "frozen rubble stays solid to queries");
}
