// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Destructible chunk structures: damage boundaries, support collapse, cascading structural failure, static queries,
//! mesh data, and the physics backend contract, against values recorded from the TypeScript engine.
//!
//! The recorded cases are the FORM DESTRUCTION example's seeded wall (`examples/form-destruction`) driven by aimed
//! hits, and a FRAMEBREAK-style building that applies the game's collapse rule (failing every ground anchor once
//! integrity drops below a threshold). Every number must match exactly: the port keeps the TypeScript `f64` arithmetic
//! and evaluation order, including V8's `Math.hypot`.

use glam::Vec3;
use joy_core::{
    RandomSource,
    SeededRandom,
};
use joy_physics::destruction::DEFAULT_CONTACT_TOLERANCE;
use joy_physics::{
    BodyOptions,
    ChunkStructure,
    ChunkSupportOptions,
    DestructibleChunkDefinition,
    DestructionError,
    PhysicsBackend,
    PhysicsCollisionShape,
    PhysicsError,
    PhysicsMutationBatch,
    PhysicsResidency,
    PhysicsWorld,
    PhysicsWorldOptions,
    RadialDamage,
    create_chunk_mesh_data,
    create_removal_mutation_batch,
};
use serde_json::{
    Value,
    json,
};

use joy_test_support::assert_matches_recording;

const REFERENCE: &str = include_str!("../fixtures/destruction-reference.json");

/// Maximum aim distance used by the FORM DESTRUCTION example, in meters.
const AIM_DISTANCE: f64 = 60.0;

/// FRAMEBREAK's collapse rule: remaining-chunk fraction below which every ground anchor fails.
const COLLAPSE_INTEGRITY: f64 = 0.55;

fn reference() -> Value {
    serde_json::from_str(REFERENCE).expect("the destruction reference is valid JSON")
}

fn definitions(value: &Value) -> Vec<DestructibleChunkDefinition> {
    serde_json::from_value(value.clone()).expect("recorded definitions deserialize")
}

fn vector(value: &Value) -> [f64; 3] {
    serde_json::from_value(value.clone()).expect("recorded vectors deserialize")
}

/// The TypeScript suite's 5×3 wall of 1×1×0.5 masonry chunks.
fn wall() -> Vec<DestructibleChunkDefinition> {
    let mut chunks = Vec::new();
    for row in 0..3 {
        for column in 0..5 {
            chunks.push(DestructibleChunkDefinition {
                id: format!("wall:{column}:{row}"),
                center: [f64::from(column) - 2.0, f64::from(row) + 0.5, 0.0],
                size: [1.0, 1.0, 0.5],
                material: "masonry".to_owned(),
                hit_points: 2.0,
            });
        }
    }
    chunks
}

fn damage(center: [f64; 3], radius: f64, amount: f64) -> RadialDamage {
    RadialDamage { center, radius, damage: amount }
}

fn ids<'a>(chunks: impl IntoIterator<Item = &'a DestructibleChunkDefinition>) -> Vec<&'a str> {
    chunks.into_iter().map(|chunk| chunk.id.as_str()).collect()
}

/// Compact per-chunk state in the recording's layout.
fn snapshot(structure: &ChunkStructure) -> Value {
    let damage = structure.damage_state();
    let support = structure.support_state();
    let mask = |flags: Vec<bool>| flags.into_iter().map(|flag| if flag { '1' } else { '0' }).collect::<String>();
    json!({
        "remaining": damage.iter().map(|chunk| chunk.remaining_hit_points).collect::<Vec<_>>(),
        "active": mask(damage.iter().map(|chunk| chunk.active).collect()),
        "anchored": mask(support.iter().map(|chunk| chunk.anchored).collect()),
        "supported": mask(support.iter().map(|chunk| chunk.supported).collect()),
        "lateral": support.iter().map(|chunk| chunk.lateral_distance).collect::<Vec<_>>(),
    })
}

fn change_summary(change: &joy_physics::ChunkChangeSet) -> Value {
    json!({
        "revision": change.revision,
        "removed": ids(&change.removed),
        "detached": ids(&change.detached),
        "activeChunkCount": change.active_chunk_count,
    })
}

fn assert_state(structure: &ChunkStructure, expected: &Value, context: &str) {
    let expected = json!({
        "remaining": expected["remaining"],
        "active": expected["active"],
        "anchored": expected["anchored"],
        "supported": expected["supported"],
        "lateral": expected["lateral"],
    });
    assert_matches_recording(&snapshot(structure), &expected, context);
}

// ---------------------------------------------------------------------------------------------------------------------
// TypeScript suite scenarios
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn chunk_damage_commits_geometry_and_traversal_queries_at_one_boundary() {
    let mut structure = ChunkStructure::new(&wall(), None).unwrap();
    let initial = structure.damage_state();
    let player_half_extents = [0.35, 0.85, 0.35];
    assert!(!structure.can_occupy([0.0, 0.85, 0.0], player_half_extents).unwrap());

    structure.queue_damage(damage([0.0, 1.0, 0.0], 1.1, 1.0)).unwrap();
    assert_eq!(structure.damage_state(), initial, "queued impacts cannot change committed diagnostics");
    assert!(structure.commit().removed.is_empty());
    let health = |structure: &ChunkStructure, id: &str| {
        structure.damage_state().into_iter().find(|chunk| chunk.chunk_id == id).unwrap()
    };
    assert_eq!(health(&structure, "wall:2:0").remaining_hit_points, 1.0);

    structure.queue_damage(damage([0.0, 1.0, 0.0], 1.1, 1.0)).unwrap();
    let change = structure.commit();
    let destroyed = health(&structure, "wall:2:0");
    assert_eq!((destroyed.remaining_hit_points, destroyed.maximum_hit_points, destroyed.active), (0.0, 2.0, false));
    assert_eq!(
        ids(&change.removed),
        ["wall:1:0", "wall:2:0", "wall:3:0", "wall:1:1", "wall:2:1", "wall:3:1", "wall:2:2"]
    );
    assert!(structure.can_occupy([0.0, 0.85, 0.0], player_half_extents).unwrap());

    let mesh = create_chunk_mesh_data(structure.active_chunks());
    assert_eq!(mesh.metadata.as_ref().unwrap()["chunkCount"], json!(8));
    assert_eq!(mesh.indices.len() / 36, 8);

    let removed: Vec<String> = change.removed.iter().map(|chunk| chunk.id.clone()).collect();
    let batch = create_removal_mutation_batch(change.revision, &removed).unwrap();
    assert!(batch.invalidates_contacts && batch.rebuilds_acceleration_structure);
}

#[test]
fn overlapping_hits_raycasts_and_reset_retain_stable_identities() {
    let definitions = wall();
    let mut structure = ChunkStructure::new(&definitions, None).unwrap();
    structure.queue_damage(damage([0.0, 0.5, 0.0], 0.4, 2.0)).unwrap();
    structure.queue_damage(damage([0.0, 0.5, 0.0], 0.4, 2.0)).unwrap();
    assert_eq!(ids(&structure.commit().removed), ["wall:2:0"]);
    assert!(structure.commit().removed.is_empty());
    let through_hole = structure.raycast([0.0, 0.5, 4.0], [0.0, 0.0, -1.0], 10.0).unwrap();
    assert!(through_hole.is_none());

    let reset = structure.reset();
    assert_eq!((reset.revision, reset.active_chunk_count), (2, 15));
    assert!(
        structure
            .damage_state()
            .iter()
            .all(|chunk| chunk.active && chunk.remaining_hit_points == chunk.maximum_hit_points)
    );
    assert_eq!(ids(structure.active_chunks()), ids(&definitions));
    let hit = structure.raycast([0.0, 0.5, 4.0], [0.0, 0.0, -1.0], 10.0).unwrap().unwrap();
    assert_eq!((hit.chunk.id.as_str(), hit.distance, hit.point), ("wall:2:0", 3.75, [0.0, 0.5, 0.25]));
}

#[test]
fn ground_support_releases_overhangs_and_islands_at_the_damage_boundary() {
    let support =
        ChunkSupportOptions { ground_height: 0.0, maximum_cantilever_distance: 1.05, contact_tolerance: None };
    let mut structure = ChunkStructure::new(&wall(), Some(support)).unwrap();
    assert!(structure.support_state().iter().all(|chunk| chunk.supported));
    for column in [1.0, 2.0, 3.0] {
        for row in [0.0, 1.0] {
            structure.queue_damage(damage([column - 2.0, row + 0.5, 0.0], 0.1, 2.0)).unwrap();
        }
    }
    assert_eq!(structure.active_chunk_count(), 15, "support and collision stay unchanged before commit");
    let opening = structure.commit();
    assert_eq!(ids(&opening.detached), ["wall:2:2"]);
    let detached = structure.damage_state().into_iter().find(|chunk| chunk.chunk_id == "wall:2:2").unwrap();
    assert_eq!(detached.remaining_hit_points, 2.0, "loss of support does not fabricate impact damage");
    assert!(structure.raycast([0.0, 2.5, 4.0], [0.0, 0.0, -1.0], 10.0).unwrap().is_none());
    let bridge = structure.support_state().into_iter().find(|chunk| chunk.chunk_id == "wall:1:2").unwrap();
    assert_eq!((bridge.supported, bridge.lateral_distance), (true, Some(1.0)));

    for x in [-2.0, 2.0] {
        structure.queue_damage(damage([x, 0.5, 0.0], 0.1, 2.0)).unwrap();
    }
    let collapse = structure.commit();
    assert_eq!(collapse.detached.len(), 6, "removing both remaining feet releases the whole upper section");
    assert_eq!(collapse.active_chunk_count, 0);
    assert!(structure.commit().removed.is_empty(), "released chunks cannot detach twice");
    structure.reset();
    assert_eq!(structure.active_chunk_count(), 15);
    assert!(structure.support_state().iter().all(|chunk| chunk.supported));
}

#[test]
fn support_excludes_corner_contacts_and_climbing_does_not_regain_reach() {
    let chunk = |id: &str, center: [f64; 3]| DestructibleChunkDefinition {
        id: id.to_owned(),
        center,
        size: [1.0, 1.0, 1.0],
        material: "masonry".to_owned(),
        hit_points: 2.0,
    };
    let support =
        ChunkSupportOptions { ground_height: 0.0, maximum_cantilever_distance: 1.05, contact_tolerance: None };
    let definitions = [
        chunk("foot", [0.0, 0.5, 0.0]),
        chunk("bridge", [1.0, 1.5, 0.0]),
        chunk("above", [1.0, 2.5, 0.0]),
        chunk("overhang", [2.0, 2.5, 0.0]),
        chunk("corner", [-1.0, 2.5, 1.0]),
        chunk("column", [0.0, 1.5, 0.0]),
    ];
    let mut structure = ChunkStructure::new(&definitions, Some(support)).unwrap();
    assert_eq!(ids(&structure.commit().detached), ["overhang", "corner"]);

    let staggered: Vec<_> = (0..7)
        .map(|index| chunk(&format!("step:{index}"), [f64::from(index) * 0.5, f64::from(index) + 0.5, 0.0]))
        .collect();
    let mut staircase = ChunkStructure::new(&staggered, Some(support)).unwrap();
    assert_eq!(ids(&staircase.commit().detached), ["step:3", "step:4", "step:5", "step:6"]);
}

// ---------------------------------------------------------------------------------------------------------------------
// Recorded parity
// ---------------------------------------------------------------------------------------------------------------------

/// The FORM DESTRUCTION example's generator: a seeded 13×6 front wall plus a U-shaped return shell.
fn form_destruction_structure(seed: i64) -> Vec<DestructibleChunkDefinition> {
    let mut random = SeededRandom::new(seed);
    let (width, height, depth) = (1.0, 1.0, 0.55);
    let (columns, rows) = (13, 6);
    let half_columns = f64::from(columns - 1) / 2.0;
    let mut chunks = Vec::new();
    for row in 0..rows {
        for column in 0..columns {
            let material = if random.next_unit() > 0.22 { "masonry" } else { "interior" };
            chunks.push(DestructibleChunkDefinition {
                id: format!("front:{column}:{row}"),
                center: [(f64::from(column) - half_columns) * width, (f64::from(row) * height) + (height / 2.0), 0.0],
                size: [width, height, depth],
                material: material.to_owned(),
                hit_points: 6.0,
            });
        }
    }
    for row in 0..4 {
        for depth_index in 1..=6 {
            for side in [-1, 1] {
                chunks.push(DestructibleChunkDefinition {
                    id: format!("side:{side}:{depth_index}:{row}"),
                    center: [
                        f64::from(side) * (half_columns * width),
                        (f64::from(row) * height) + (height / 2.0),
                        -f64::from(depth_index) * depth,
                    ],
                    size: [depth, height, depth],
                    material: "frame".to_owned(),
                    hit_points: 10.0,
                });
            }
        }
    }
    chunks
}

#[test]
fn form_destruction_wall_matches_the_typescript_example_hit_for_hit() {
    let reference = reference();
    let case = &reference["cases"][0];
    assert_eq!(case["name"], "form-destruction");

    // The seeded generator is reproduced exactly from the shared random stream.
    let generated = form_destruction_structure(7319);
    assert_eq!(generated, definitions(&case["definitions"]));
    let support: ChunkSupportOptions = serde_json::from_value(case["support"].clone()).unwrap();
    let mut structure = ChunkStructure::new(&generated, Some(support)).unwrap();

    for (index, step) in case["steps"].as_array().unwrap().iter().enumerate() {
        let context = format!("step {index} ({})", step["kind"]);
        match step["kind"].as_str().unwrap() {
            "initial" => assert_state(&structure, step, &context),
            "hit" => {
                let ray = structure.raycast(vector(&step["origin"]), vector(&step["direction"]), AIM_DISTANCE).unwrap();
                let recorded =
                    ray.map(|hit| json!({"chunk": hit.chunk.id, "distance": hit.distance, "point": hit.point}));
                assert_matches_recording(&recorded, &step["ray"], &context);
                if let Some(hit) = ray {
                    let point = hit.point;
                    let amount = damage(point, step["radius"].as_f64().unwrap(), step["damage"].as_f64().unwrap());
                    structure.queue_damage(amount).unwrap();
                }
                assert_matches_recording(&change_summary(&structure.commit()), &step["change"], &context);
                assert_state(&structure, &step["state"], &context);
            }
            "mesh" => {
                let mesh = create_chunk_mesh_data(structure.active_chunks());
                let colors = mesh.colors.as_deref().unwrap();
                let summary = json!({
                    "positionsLength": mesh.positions.len(),
                    "indicesLength": mesh.indices.len(),
                    "metadata": mesh.metadata,
                    "positionSum": mesh.positions.iter().sum::<f64>(),
                    "colorSum": colors.iter().sum::<f64>(),
                    "firstPositions": &mesh.positions[..36],
                    "firstColors": &colors[..36],
                    "lastIndices": &mesh.indices[mesh.indices.len() - 12..],
                });
                let mut expected = step.clone();
                expected.as_object_mut().unwrap().remove("kind");
                assert_matches_recording(&summary, &expected, &context);
            }
            "reset" => {
                assert_matches_recording(&change_summary(&structure.reset()), &step["change"], &context);
                assert_state(&structure, &step["state"], &context);
            }
            other => panic!("unexpected step {other}"),
        }
    }
}

#[test]
fn framebreak_building_cascades_through_support_loss_and_the_collapse_rule() {
    let reference = reference();
    let case = &reference["cases"][1];
    assert_eq!(case["name"], "framebreak-tower");
    let definitions = definitions(&case["definitions"]);
    let support: ChunkSupportOptions = serde_json::from_value(case["support"].clone()).unwrap();
    assert_eq!(support.contact_tolerance, None, "the default contact tolerance applies");
    let mut structure = ChunkStructure::new(&definitions, Some(support)).unwrap();
    let initial_count = definitions.len() as f64;
    let mut collapsed = false;

    for (index, step) in case["steps"].as_array().unwrap().iter().enumerate() {
        let context = format!("step {index}");
        if step["kind"] == "initial" {
            assert_state(&structure, step, &context);
            continue;
        }
        let blast = &step["blast"];
        structure
            .queue_damage(damage(
                vector(&blast["center"]),
                blast["radius"].as_f64().unwrap(),
                blast["damage"].as_f64().unwrap(),
            ))
            .unwrap();
        let first = structure.commit();
        assert_matches_recording(&change_summary(&first), &step["change"], &context);

        // The game's structural-failure rule runs a second commit at the same boundary.
        let integrity = first.active_chunk_count as f64 / initial_count;
        if !collapsed && first.active_chunk_count > 0 && integrity < COLLAPSE_INTEGRITY {
            collapsed = true;
            let anchors: Vec<DestructibleChunkDefinition> = structure
                .active_chunks()
                .filter(|chunk| chunk.center[1] - (chunk.size[1] / 2.0) <= 0.001)
                .cloned()
                .collect();
            assert_matches_recording(&ids(&anchors), &step["collapseAnchors"], &context);
            for anchor in &anchors {
                structure.queue_damage(damage(anchor.center, 0.01, anchor.hit_points * 100.0)).unwrap();
            }
            assert_matches_recording(&change_summary(&structure.commit()), &step["collapse"], &context);
        } else {
            assert!(step.get("collapse").is_none(), "{context}: the collapse rule fired in TypeScript only");
        }
        assert_state(&structure, &step["state"], &context);

        let occupancy: Vec<bool> = [[40.0, 1.0, -12.0], [36.25, 1.0, -14.5], [46.25, 14.4, -12.0]]
            .into_iter()
            .map(|center| structure.can_occupy(center, [0.6, 0.9, 0.6]).unwrap())
            .collect();
        assert_matches_recording(&occupancy, &step["occupancy"], &context);
    }
    assert!(collapsed);
}

#[test]
fn contact_tolerance_and_corner_rules_match_recorded_support() {
    let reference = reference();
    let case = &reference["cases"][2];
    let support: ChunkSupportOptions = serde_json::from_value(case["support"].clone()).unwrap();
    assert_ne!(support.contact_tolerance, Some(DEFAULT_CONTACT_TOLERANCE));
    let mut structure = ChunkStructure::new(&definitions(&case["definitions"]), Some(support)).unwrap();
    assert_state(&structure, &case["steps"][0], "initial");
    assert_matches_recording(&change_summary(&structure.commit()), &case["steps"][1]["change"], "commit");
    assert_state(&structure, &case["steps"][1]["state"], "commit");
}

#[test]
fn chunk_mesh_data_and_removal_batches_match_typescript_records() {
    let reference = reference();
    let wall_definitions = definitions(&reference["wall"]["definitions"]);
    let mesh = create_chunk_mesh_data(&wall_definitions);
    assert_matches_recording(&mesh, &reference["wall"]["mesh"], "wall mesh");

    let removed = vec!["wall:1:0".to_owned(), "wall:2:0".to_owned()];
    assert_matches_recording(&create_removal_mutation_batch(4, &removed).unwrap(), &reference["batch"], "batch");
    assert_matches_recording(&create_removal_mutation_batch(0, &[]).unwrap(), &reference["emptyBatch"], "empty batch");
    let error = create_removal_mutation_batch(1, &["ok".to_owned(), String::new()]).unwrap_err();
    assert_eq!(error.to_string(), reference["batchError"].as_str().unwrap());
}

#[test]
fn invalid_definitions_damage_and_queries_report_typescript_messages() {
    let reference = reference();
    let errors = &reference["errors"];
    let message = |error: DestructionError| error.to_string();
    let chunks = wall();
    let mut invalid = chunks[0].clone();

    let duplicate = ChunkStructure::new(&[chunks[0].clone(), chunks[0].clone()], None).unwrap_err();
    assert_eq!(message(duplicate), errors["duplicate"].as_str().unwrap());
    invalid.material.clear();
    assert_eq!(message(ChunkStructure::new(&[invalid], None).unwrap_err()), errors["identity"].as_str().unwrap());
    let mut flat = chunks[0].clone();
    flat.size[1] = 0.0;
    assert_eq!(message(ChunkStructure::new(&[flat], None).unwrap_err()), errors["dimensions"].as_str().unwrap());
    let support =
        ChunkSupportOptions { ground_height: 0.0, maximum_cantilever_distance: -1.0, contact_tolerance: None };
    assert_eq!(message(ChunkStructure::new(&chunks, Some(support)).unwrap_err()), errors["support"].as_str().unwrap());

    let mut structure = ChunkStructure::new(&chunks, None).unwrap();
    let center_error = structure.queue_damage(damage([0.0, f64::NAN, 0.0], 1.0, 1.0)).unwrap_err();
    assert_eq!(message(center_error), errors["damageCenter"].as_str().unwrap());
    let amount_error = structure.queue_damage(damage([0.0; 3], 0.0, 1.0)).unwrap_err();
    assert_eq!(message(amount_error), errors["damageAmount"].as_str().unwrap());
    let ray_error = structure.raycast([0.0; 3], [0.0, 0.0, 1.0], 0.0).unwrap_err();
    assert_eq!(message(ray_error), errors["raycast"].as_str().unwrap());
    assert_eq!(structure.raycast([f64::NAN, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0), Err(DestructionError::InvalidRaycast));
    let occupancy_error = structure.can_occupy([0.0; 3], [1.0, 0.0, 1.0]).unwrap_err();
    assert_eq!(message(occupancy_error), errors["occupancy"].as_str().unwrap());
    assert_eq!(structure.commit().revision, 0, "rejected damage never reaches the structure");
}

// ---------------------------------------------------------------------------------------------------------------------
// Physics backend contract
// ---------------------------------------------------------------------------------------------------------------------

fn shape(chunk: &DestructibleChunkDefinition) -> PhysicsCollisionShape {
    PhysicsCollisionShape {
        id: chunk.id.clone(),
        center: chunk.center,
        half_extents: chunk.size.map(|value| value / 2.0),
    }
}

#[test]
fn physics_world_keeps_chunk_collision_in_step_with_committed_damage() {
    let mut physics = PhysicsWorld::new(PhysicsWorldOptions::default()).unwrap();
    assert_eq!(physics.residency(), PhysicsResidency::Cpu);
    let mut structure = ChunkStructure::new(&wall(), None).unwrap();
    let initial = PhysicsMutationBatch {
        sequence: structure.revision(),
        removed_shape_ids: Vec::new(),
        added_shapes: structure.active_chunks().map(shape).collect(),
        invalidates_contacts: false,
        rebuilds_acceleration_structure: true,
    };
    physics.apply_mutations(&initial).unwrap();
    assert_eq!(physics.active_body_count(), 15);

    // Queries see resident shapes and agree with the structure's own ray test.
    let hit = physics.query([0.0, 0.5, 4.0], [0.0, 0.0, -1.0], 10.0).unwrap();
    assert_eq!(hit.shape_id, "wall:2:0");
    assert!((hit.distance - 3.75).abs() < 1e-5 && (hit.point[2] - 0.25).abs() < 1e-5);

    // A committed removal becomes a batch at the same boundary; the query then passes through the hole.
    structure.queue_damage(damage([0.0, 0.5, 0.0], 0.4, 2.0)).unwrap();
    let change = structure.commit();
    let removed: Vec<String> = change.removed.iter().map(|chunk| chunk.id.clone()).collect();
    physics.apply_mutations(&create_removal_mutation_batch(change.revision, &removed).unwrap()).unwrap();
    assert!(physics.query([0.0, 0.5, 4.0], [0.0, 0.0, -1.0], 10.0).is_none());
    assert_eq!(physics.resident_shape_ids().count(), 14);

    // Stale, duplicate, and malformed batches change nothing.
    let stale = create_removal_mutation_batch(0, &["wall:0:0".to_owned()]).unwrap();
    assert_eq!(physics.apply_mutations(&stale), Err(PhysicsError::StaleMutationBatch { sequence: 0, last: 1 }));
    let duplicate = PhysicsMutationBatch {
        sequence: 2,
        removed_shape_ids: Vec::new(),
        added_shapes: vec![shape(&wall()[0])],
        invalidates_contacts: false,
        rebuilds_acceleration_structure: true,
    };
    assert_eq!(physics.apply_mutations(&duplicate), Err(PhysicsError::DuplicateShape("wall:0:0".to_owned())));
    let mut flat = shape(&wall()[1]);
    flat.id = "debris:0".to_owned();
    flat.half_extents[2] = 0.0;
    let malformed = PhysicsMutationBatch { added_shapes: vec![flat], ..duplicate };
    assert_eq!(physics.apply_mutations(&malformed), Err(PhysicsError::InvalidShape("debris:0".to_owned())));
    assert_eq!(physics.resident_shape_ids().count(), 14);

    // Resident shapes are static colliders: debris dropped onto the wall comes to rest on top of it.
    let debris = physics
        .create_body(BodyOptions {
            mass: 1.0,
            half_extents: Vec3::splat(0.2),
            position: Vec3::new(-1.0, 4.0, 0.0),
            ..BodyOptions::default()
        })
        .unwrap();
    for _ in 0..180 {
        PhysicsWorld::step(&mut physics, 1.0 / 60.0).unwrap();
    }
    let resting = physics.body_transform(debris).unwrap().position;
    assert!((resting.y - 3.2).abs() < 0.05, "debris rests on the wall top, at {resting}");
}
