// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Structural stress and undermining: load paths through a small masonry wall, staged and delayed overload failure,
//! relief that cancels a countdown, and terrain craters that remove anchors under a structure.
//!
//! The wall is five columns by three rows of 1 x 1 x 0.5 chunks with density 2, so every chunk weighs exactly 1 and
//! every face contact is 0.5 square units. Expected loads and ratios are exact: stress keeps a fixed `f64` order.

use joy_core::math::DVec3;
use joy_physics::destruction::{
    ChunkMaterialStrength,
    ChunkStressOptions,
    ChunkStressStage,
    ChunkStressState,
};
use joy_physics::{
    ChunkStructure,
    ChunkSupportOptions,
    DestructibleChunkDefinition,
    DestructionError,
    RadialDamage,
};
use joy_terrain::TerrainCrater;

/// Masonry strength: an intact bottom chunk under a full column of three sits at half its capacity.
const MASONRY_STRENGTH: f64 = 12.0;

/// Seconds an overloaded chunk holds before failing.
const FAILURE_DELAY_SECONDS: f64 = 2.0;

/// Bearing tolerance for ground surfaces, in world units.
const BEARING_TOLERANCE: f64 = 0.05;

fn wall() -> Vec<DestructibleChunkDefinition> {
    let mut chunks = Vec::new();
    for row in 0..3 {
        for column in 0..5 {
            chunks.push(DestructibleChunkDefinition {
                id: format!("wall:{column}:{row}"),
                center: DVec3::new(f64::from(column) - 2.0, f64::from(row) + 0.5, 0.0),
                size: DVec3::new(1.0, 1.0, 0.5),
                material: "masonry".to_owned(),
                hit_points: 2.0,
                shape: None,
            });
        }
    }
    chunks
}

fn support() -> ChunkSupportOptions {
    ChunkSupportOptions {
        ground_height: 0.0,
        maximum_cantilever_distance: 1.05,
        contact_tolerance: None,
    }
}

fn stress() -> ChunkStressOptions {
    ChunkStressOptions {
        materials: vec![ChunkMaterialStrength {
            material: "masonry".to_owned(),
            density: 2.0,
            strength: MASONRY_STRENGTH,
        }],
        default_density: 1.0,
        default_strength: 1.0,
        lateral_strength_fraction: 0.2,
        warning_ratio: 0.7,
        failure_delay_seconds: FAILURE_DELAY_SECONDS,
    }
}

fn stressed_wall() -> ChunkStructure {
    ChunkStructure::new(&wall(), Some(support())).unwrap().with_stress(stress()).unwrap()
}

fn hit(structure: &mut ChunkStructure, x: f64, y: f64, amount: f64) {
    let damage = RadialDamage {
        center: DVec3::new(x, y, 0.0),
        radius: 0.1,
        damage: amount,
    };
    structure.queue_damage(damage).unwrap();
}

fn chunk_stress(structure: &ChunkStructure, chunk_id: &str) -> ChunkStressState {
    structure.stress_state().into_iter().find(|state| state.chunk_id == chunk_id).unwrap()
}

fn ids(chunks: &[DestructibleChunkDefinition]) -> Vec<&str> {
    chunks.iter().map(|chunk| chunk.id.as_str()).collect()
}

fn bearing_chunk(id: &str, center: [f64; 3], size: [f64; 3]) -> DestructibleChunkDefinition {
    DestructibleChunkDefinition {
        id: id.to_owned(),
        center: DVec3::from_array(center),
        size: DVec3::from_array(size),
        material: id.to_owned(),
        hit_points: 1.0,
        shape: None,
    }
}

fn bearing_structure(chunks: &[DestructibleChunkDefinition]) -> ChunkStructure {
    let options = ChunkStressOptions {
        materials: vec![ChunkMaterialStrength {
            material: "beam".to_owned(),
            density: 1.0,
            strength: 2.0,
        }],
        default_density: 1.0,
        default_strength: 100.0,
        ..stress()
    };
    let support = ChunkSupportOptions {
        maximum_cantilever_distance: 10.0,
        ..support()
    };
    ChunkStructure::new(chunks, Some(support)).unwrap().with_stress(options).unwrap()
}

#[test]
fn every_grounded_bearing_carries_a_beam_regardless_of_chunk_order() {
    let left = bearing_chunk("left", [0.0, 0.5, 0.0], [1.0; 3]);
    let right = bearing_chunk("right", [1.0, 0.5, 0.0], [1.0; 3]);
    let beam = bearing_chunk("beam", [0.0, 1.5, 0.0], [3.0, 1.0, 1.0]);
    for chunks in
        [[left.clone(), beam.clone(), right.clone()], [left.clone(), right.clone(), beam.clone()], [beam, right, left]]
    {
        let mut structure = bearing_structure(&chunks);
        let beam = chunk_stress(&structure, "beam");
        assert_eq!((beam.load, beam.capacity, beam.ratio), (3.0, 4.0, 0.75));
        assert_eq!(chunk_stress(&structure, "left").load, 2.5);
        assert_eq!(chunk_stress(&structure, "right").load, 2.5);
        assert!(!structure.advance_stress(FAILURE_DELAY_SECONDS).unwrap());
        assert!(structure.commit().removed.is_empty(), "reordering must not destroy a safely supported beam");
    }
}

#[test]
fn later_reached_piers_receive_and_propagate_their_share_of_a_beam() {
    // The aligned left stack reaches the beam before the offset right pier. Both pier faces still bear its mass.
    let chunks = vec![
        bearing_chunk("left-anchor", [0.0, 0.5, 0.0], [1.0; 3]),
        bearing_chunk("left-pier", [0.0, 1.5, 0.0], [1.0; 3]),
        bearing_chunk("beam", [0.0, 2.5, 0.0], [4.0, 1.0, 1.0]),
        bearing_chunk("right-anchor", [2.0, 0.5, 0.0], [1.0; 3]),
        bearing_chunk("right-pier", [1.25, 1.5, 0.0], [1.5, 1.0, 1.0]),
    ];
    for chunks in [chunks.clone(), chunks.into_iter().rev().collect()] {
        let mut structure = bearing_structure(&chunks);
        let beam = chunk_stress(&structure, "beam");
        assert_eq!((beam.load, beam.capacity, beam.ratio), (4.0, 5.0, 0.8));
        assert!((chunk_stress(&structure, "left-pier").load - 2.6).abs() < 1e-12);
        assert!((chunk_stress(&structure, "right-pier").load - 3.9).abs() < 1e-12);
        assert!((chunk_stress(&structure, "left-anchor").load - 3.6).abs() < 1e-12);
        assert!((chunk_stress(&structure, "right-anchor").load - 4.9).abs() < 1e-12);
        assert!(!structure.advance_stress(FAILURE_DELAY_SECONDS).unwrap());
        assert!(structure.commit().removed.is_empty());
    }
}

#[test]
fn circular_bearings_route_their_load_through_grounded_side_contacts() {
    // The slab hangs from a high ground anchor. Its underside touches a suspended pedestal, whose only support
    // comes sideways through the tall brace and back through the slab. Counting that bearing would make a cycle.
    let chunks = vec![
        bearing_chunk("anchor", [-1.5, 3.0, 0.0], [1.0; 3]),
        bearing_chunk("slab", [0.0, 3.0, 0.0], [2.0, 1.0, 2.0]),
        bearing_chunk("pedestal", [0.5, 2.0, 0.0], [1.0; 3]),
        bearing_chunk("brace", [1.5, 2.5, 0.0], [1.0, 2.0, 1.0]),
    ];
    for chunks in [chunks.clone(), chunks.into_iter().rev().collect()] {
        let mut structure = bearing_structure(&chunks);
        structure.set_ground_surface(0.0, |x, _| if x < -1.0 { 2.5 } else { -10.0 }).unwrap();
        assert!(structure.commit().detached.is_empty());
        assert!(structure.stress_states().iter().all(|state| state.capacity > 0.0 && state.ratio.is_finite()));
        assert_eq!(chunk_stress(&structure, "anchor").load, 8.0, "all mass must reach the sole anchor once");
        assert_eq!(chunk_stress(&structure, "slab").load, 7.0);
        assert_eq!(chunk_stress(&structure, "brace").load, 3.0);
        assert_eq!(chunk_stress(&structure, "pedestal").load, 1.0);
    }
}

#[test]
fn columns_carry_their_load_and_a_weakened_base_fails_only_after_its_delay() {
    let mut structure = stressed_wall();
    let base = chunk_stress(&structure, "wall:2:0");
    assert_eq!((base.load, base.capacity, base.ratio), (3.0, 6.0, 0.5), "a base carries its whole column");
    assert_eq!(chunk_stress(&structure, "wall:2:2").load, 1.0, "the top chunk carries only itself");
    assert!(structure.stress_state().iter().all(|state| state.stage == ChunkStressStage::Stable));

    // Weakening the base lowers its capacity with its health; the column above is now too heavy for it.
    hit(&mut structure, 0.0, 0.5, 1.2);
    let weakened = structure.commit();
    assert!(weakened.removed.is_empty(), "damage short of breaking the chunk removes nothing yet");
    let base = chunk_stress(&structure, "wall:2:0");
    assert_eq!(base.stage, ChunkStressStage::Failing);
    assert!((base.ratio - 1.25).abs() < 1e-12);
    assert_eq!(base.seconds_until_failure, Some(FAILURE_DELAY_SECONDS));
    assert_eq!(structure.failing_chunk_count(), 1);

    // The delay is the telegraph: nothing falls until it runs out.
    assert!(!structure.advance_stress(1.5).unwrap());
    assert!(structure.commit().removed.is_empty(), "committing mid-countdown keeps the countdown");
    assert_eq!(chunk_stress(&structure, "wall:2:0").seconds_until_failure, Some(0.5));
    assert!(structure.advance_stress(0.5).unwrap());
    let failure = structure.commit();
    assert_eq!(ids(&failure.overloaded), ["wall:2:0"]);
    assert_eq!(ids(&failure.removed), ["wall:2:0"]);
    assert!(failure.detached.is_empty(), "the column above stays up on its neighbors within the cantilever");
    let failed = structure.damage_state().into_iter().find(|state| state.chunk_id == "wall:2:0").unwrap();
    assert_eq!(failed.remaining_hit_points, 0.8, "overload failure keeps the chunk's remaining health");

    // The hanging column now bears sideways at reduced strength (strained), and its neighbors take half each.
    let hanging = chunk_stress(&structure, "wall:2:1");
    assert_eq!((hanging.load, hanging.stage), (2.0, ChunkStressStage::Strained));
    assert!((hanging.capacity - 2.4).abs() < 1e-12);
    assert_eq!(chunk_stress(&structure, "wall:1:0").load, 4.0);
    assert_eq!(chunk_stress(&structure, "wall:2:0").stage, ChunkStressStage::Stable, "inactive chunks are stable");

    structure.reset();
    assert_eq!(structure.failing_chunk_count(), 0);
    assert_eq!(chunk_stress(&structure, "wall:2:0").ratio, 0.5);
}

#[test]
fn relieving_an_overloaded_chunk_cancels_its_countdown() {
    let mut structure = stressed_wall();
    hit(&mut structure, 0.0, 0.5, 1.2);
    structure.commit();
    assert!(!structure.advance_stress(1.0).unwrap());

    // Breaking the load above the weakened base removes the reason it was failing.
    hit(&mut structure, 0.0, 1.5, 2.0);
    hit(&mut structure, 0.0, 2.5, 2.0);
    let relief = structure.commit();
    assert_eq!(ids(&relief.removed), ["wall:2:1", "wall:2:2"]);
    let base = chunk_stress(&structure, "wall:2:0");
    assert_eq!((base.stage, base.seconds_until_failure), (ChunkStressStage::Stable, None));
    assert!(!structure.advance_stress(10.0).unwrap());
    assert!(structure.commit().removed.is_empty());
}

#[test]
fn craters_undermine_anchors_and_release_or_overload_what_they_held() {
    let mut structure = stressed_wall();
    let crater = TerrainCrater {
        center: [2.0, 0.0],
        radius: 1.6,
        depth: 0.5,
        rim_height: 0.0,
    };
    let changed = structure.set_ground_surface(BEARING_TOLERANCE, |x, z| crater.offset_at(x, z)).unwrap();
    assert!(changed);
    assert!(
        structure.support_state().iter().all(|state| state.supported),
        "undermining reaches support and stress at the next commit"
    );

    let collapse = structure.commit();
    assert_eq!(ids(&collapse.detached), ["wall:4:0", "wall:4:1", "wall:4:2"], "beyond the cantilever, columns fall");
    let undermined = structure.support_state().into_iter().find(|state| state.chunk_id == "wall:3:0").unwrap();
    assert_eq!((undermined.anchored, undermined.supported), (false, true));
    let hanging = chunk_stress(&structure, "wall:3:0");
    assert_eq!(hanging.stage, ChunkStressStage::Failing, "an undermined column hangs from one side face");
    assert_eq!(hanging.load, 3.0);

    // Restoring the ground and the structure brings every anchor back.
    assert!(structure.set_ground_surface(BEARING_TOLERANCE, |_, _| 0.0).unwrap());
    assert!(!structure.set_ground_surface(BEARING_TOLERANCE, |_, _| 0.0).unwrap(), "unchanged ground is no change");
    structure.reset();
    assert!(structure.support_state().iter().all(|state| state.supported));
    assert!(structure.stress_state().iter().all(|state| state.stage == ChunkStressStage::Stable));
}

#[test]
fn stress_and_ground_surfaces_reject_invalid_configuration() {
    let unsupported = ChunkStructure::new(&wall(), None).unwrap();
    assert_eq!(unsupported.clone().with_stress(stress()).unwrap_err(), DestructionError::SupportDisabled);
    assert_eq!(
        unsupported.clone().set_ground_surface(BEARING_TOLERANCE, |_, _| 0.0).unwrap_err(),
        DestructionError::SupportDisabled
    );

    let supported = ChunkStructure::new(&wall(), Some(support())).unwrap();
    let mut duplicate = stress();
    duplicate.materials.push(duplicate.materials[0].clone());
    let mut warning = stress();
    warning.warning_ratio = 1.0;
    let mut delay = stress();
    delay.failure_delay_seconds = f64::NAN;
    for options in [duplicate, warning, delay] {
        assert_eq!(supported.clone().with_stress(options).unwrap_err(), DestructionError::InvalidStressOptions);
    }

    let mut structure = stressed_wall();
    assert_eq!(structure.advance_stress(-1.0).unwrap_err(), DestructionError::InvalidElapsedTime);
    assert_eq!(structure.set_ground_surface(-1.0, |_, _| 0.0).unwrap_err(), DestructionError::InvalidSupportOptions);
    assert_eq!(structure.stress_options().map(|options| options.chunk_mass(&wall()[0])), Some(1.0));
}
