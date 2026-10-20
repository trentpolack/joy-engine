// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Destructible chunk structures: damage boundaries, support collapse, cascading structural failure, static queries,
//! mesh data, and the physics backend contract, checked against the golden recording
//! `tests/fixtures/destruction-reference.json`.
//!
//! The recorded cases are the FORM DESTRUCTION example's seeded wall (`examples/form-destruction`) driven by aimed
//! hits, and a FRAMEBREAK-style building that applies the game's collapse rule (failing every ground anchor once
//! integrity drops below a threshold). Every number must match exactly: destruction keeps a fixed `f64` operation order
//! (including its deterministic `hypot`), so seeded runs reproduce bit for bit. Re-record with `JOY_UPDATE_FIXTURES=1`
//! when destruction changes on purpose.

use joy_core::math::{
    DVec3,
    Vec3,
};
use joy_core::{
    RandomSource,
    SeededRandom,
};
use joy_physics::destruction::DEFAULT_CONTACT_TOLERANCE;
use joy_physics::{
    BodyOptions,
    ChunkChangeSet,
    ChunkRaycastHit,
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
use joy_test_support::{
    GoldenFile,
    Tolerance,
    golden_file,
};
use serde_json::{
    Value,
    json,
};

/// Maximum aim distance used by the FORM DESTRUCTION example, in meters.
const AIM_DISTANCE: f64 = 60.0;

/// FRAMEBREAK's collapse rule: remaining-chunk fraction below which every ground anchor fails.
const COLLAPSE_INTEGRITY: f64 = 0.55;

/// Leading position and color values recorded in the mesh summary.
const MESH_LEADING_VALUES: usize = 36;

/// Trailing indices recorded in the mesh summary.
const MESH_TRAILING_INDICES: usize = 12;

fn golden() -> GoldenFile {
    golden_file!("destruction-reference.json")
}

fn definitions(value: &Value) -> Vec<DestructibleChunkDefinition> {
    serde_json::from_value(value.clone()).expect("recorded definitions deserialize")
}

fn vector(value: &Value) -> DVec3 {
    serde_json::from_value(value.clone()).expect("recorded vectors deserialize")
}

/// A serializable engine value as recorded JSON.
fn to_record(value: &impl serde::Serialize) -> Value {
    serde_json::to_value(value).expect("destruction records serialize")
}

/// A 5×3 test wall of 1×1×0.5 masonry chunks.
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
            });
        }
    }
    chunks
}

fn damage(center: DVec3, radius: f64, amount: f64) -> RadialDamage {
    RadialDamage {
        center,
        radius,
        damage: amount,
    }
}

fn ids<'a>(chunks: impl IntoIterator<Item = &'a DestructibleChunkDefinition>) -> Vec<&'a str> {
    chunks.into_iter().map(|chunk| chunk.id.as_str()).collect()
}

// ---------------------------------------------------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------------------------------------------------

/// Compact per-chunk state, in definition order: hit points and lateral distances as arrays, flags as `0`/`1` masks.
fn state_record(structure: &ChunkStructure) -> Value {
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

/// One commit or reset: its revision, the chunk ids it removed and detached, and the chunks left active.
fn change_record(change: &ChunkChangeSet) -> Value {
    json!({
        "revision": change.revision,
        "removed": ids(&change.removed),
        "detached": ids(&change.detached),
        "activeChunkCount": change.active_chunk_count,
    })
}

/// The chunk an aimed ray hits, or `null` when it misses.
fn ray_record(hit: Option<&ChunkRaycastHit>) -> Value {
    hit.map_or(Value::Null, |hit| json!({"chunk": hit.chunk.id, "distance": hit.distance, "point": hit.point}))
}

/// Sizes, sums, and leading and trailing values of the active chunks' mesh data.
fn mesh_summary_record(structure: &ChunkStructure) -> Value {
    let mesh = create_chunk_mesh_data(structure.active_chunks());
    // The recording keeps the flat XYZ and RGB layout; colors read back as their shortest decimals (`0.18`, not the
    // `f32` value's full expansion).
    let positions: Vec<f64> = mesh.positions.iter().flat_map(|position| position.to_array()).collect();
    let colors: Vec<f64> = mesh
        .colors
        .as_deref()
        .expect("chunk meshes carry colors")
        .iter()
        .flat_map(|color| color.rgb_array())
        .map(|channel| channel.to_string().parse().unwrap_or(f64::from(channel)))
        .collect();
    json!({
        "positionsLength": positions.len(),
        "indicesLength": mesh.indices.len(),
        "metadata": mesh.metadata,
        "positionSum": positions.iter().sum::<f64>(),
        "colorSum": colors.iter().sum::<f64>(),
        "firstPositions": &positions[..MESH_LEADING_VALUES],
        "firstColors": &colors[..MESH_LEADING_VALUES],
        "lastIndices": &mesh.indices[mesh.indices.len() - MESH_TRAILING_INDICES..],
    })
}

/// Write each field of `record` into the step object `step`, for steps that hold a record's fields beside their own.
fn write_fields(step: &mut Value, record: Value) {
    let Value::Object(fields) = record else {
        panic!("step records are objects");
    };
    for (key, value) in fields {
        step[key.as_str()] = value;
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Damage, support, and query scenarios
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn chunk_damage_commits_geometry_and_traversal_queries_at_one_boundary() {
    let mut structure = ChunkStructure::new(&wall(), None).unwrap();
    let initial = structure.damage_state();
    let player_half_extents = DVec3::new(0.35, 0.85, 0.35);
    assert!(!structure.can_occupy(DVec3::new(0.0, 0.85, 0.0), player_half_extents).unwrap());

    structure.queue_damage(damage(DVec3::new(0.0, 1.0, 0.0), 1.1, 1.0)).unwrap();
    assert_eq!(structure.damage_state(), initial, "queued impacts cannot change committed diagnostics");
    assert!(structure.commit().removed.is_empty());
    let health = |structure: &ChunkStructure, id: &str| {
        structure.damage_state().into_iter().find(|chunk| chunk.chunk_id == id).unwrap()
    };
    assert_eq!(health(&structure, "wall:2:0").remaining_hit_points, 1.0);

    structure.queue_damage(damage(DVec3::new(0.0, 1.0, 0.0), 1.1, 1.0)).unwrap();
    let change = structure.commit();
    let destroyed = health(&structure, "wall:2:0");
    assert_eq!((destroyed.remaining_hit_points, destroyed.maximum_hit_points, destroyed.active), (0.0, 2.0, false));
    assert_eq!(
        ids(&change.removed),
        ["wall:1:0", "wall:2:0", "wall:3:0", "wall:1:1", "wall:2:1", "wall:3:1", "wall:2:2"]
    );
    assert!(structure.can_occupy(DVec3::new(0.0, 0.85, 0.0), player_half_extents).unwrap());

    let mesh = create_chunk_mesh_data(structure.active_chunks());
    assert_eq!(mesh.metadata["chunkCount"], json!(8));
    assert_eq!(mesh.indices.len() / 36, 8);

    let removed: Vec<String> = change.removed.iter().map(|chunk| chunk.id.clone()).collect();
    let batch = create_removal_mutation_batch(change.revision, &removed).unwrap();
    assert!(batch.invalidates_contacts && batch.rebuilds_acceleration_structure);
}

#[test]
fn overlapping_hits_raycasts_and_reset_retain_stable_identities() {
    let definitions = wall();
    let mut structure = ChunkStructure::new(&definitions, None).unwrap();
    structure.queue_damage(damage(DVec3::new(0.0, 0.5, 0.0), 0.4, 2.0)).unwrap();
    structure.queue_damage(damage(DVec3::new(0.0, 0.5, 0.0), 0.4, 2.0)).unwrap();
    assert_eq!(ids(&structure.commit().removed), ["wall:2:0"]);
    assert!(structure.commit().removed.is_empty());
    let through_hole = structure.raycast(DVec3::new(0.0, 0.5, 4.0), DVec3::new(0.0, 0.0, -1.0), 10.0).unwrap();
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
    let hit = structure.raycast(DVec3::new(0.0, 0.5, 4.0), DVec3::new(0.0, 0.0, -1.0), 10.0).unwrap().unwrap();
    assert_eq!((hit.chunk.id.as_str(), hit.distance, hit.point), ("wall:2:0", 3.75, DVec3::new(0.0, 0.5, 0.25)));
}

#[test]
fn ground_support_releases_overhangs_and_islands_at_the_damage_boundary() {
    let support = ChunkSupportOptions {
        ground_height: 0.0,
        maximum_cantilever_distance: 1.05,
        contact_tolerance: None,
    };
    let mut structure = ChunkStructure::new(&wall(), Some(support)).unwrap();
    assert!(structure.support_state().iter().all(|chunk| chunk.supported));
    for column in [1.0, 2.0, 3.0] {
        for row in [0.0, 1.0] {
            structure.queue_damage(damage(DVec3::new(column - 2.0, row + 0.5, 0.0), 0.1, 2.0)).unwrap();
        }
    }
    assert_eq!(structure.active_chunk_count(), 15, "support and collision stay unchanged before commit");
    let opening = structure.commit();
    assert_eq!(ids(&opening.detached), ["wall:2:2"]);
    let detached = structure.damage_state().into_iter().find(|chunk| chunk.chunk_id == "wall:2:2").unwrap();
    assert_eq!(detached.remaining_hit_points, 2.0, "loss of support does not fabricate impact damage");
    assert!(structure.raycast(DVec3::new(0.0, 2.5, 4.0), DVec3::new(0.0, 0.0, -1.0), 10.0).unwrap().is_none());
    let bridge = structure.support_state().into_iter().find(|chunk| chunk.chunk_id == "wall:1:2").unwrap();
    assert_eq!((bridge.supported, bridge.lateral_distance), (true, Some(1.0)));

    for x in [-2.0, 2.0] {
        structure.queue_damage(damage(DVec3::new(x, 0.5, 0.0), 0.1, 2.0)).unwrap();
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
    let chunk = |id: &str, center: DVec3| DestructibleChunkDefinition {
        id: id.to_owned(),
        center,
        size: DVec3::new(1.0, 1.0, 1.0),
        material: "masonry".to_owned(),
        hit_points: 2.0,
    };
    let support = ChunkSupportOptions {
        ground_height: 0.0,
        maximum_cantilever_distance: 1.05,
        contact_tolerance: None,
    };
    let definitions = [
        chunk("foot", DVec3::new(0.0, 0.5, 0.0)),
        chunk("bridge", DVec3::new(1.0, 1.5, 0.0)),
        chunk("above", DVec3::new(1.0, 2.5, 0.0)),
        chunk("overhang", DVec3::new(2.0, 2.5, 0.0)),
        chunk("corner", DVec3::new(-1.0, 2.5, 1.0)),
        chunk("column", DVec3::new(0.0, 1.5, 0.0)),
    ];
    let mut structure = ChunkStructure::new(&definitions, Some(support)).unwrap();
    assert_eq!(ids(&structure.commit().detached), ["overhang", "corner"]);

    let staggered: Vec<_> = (0..7)
        .map(|index| chunk(&format!("step:{index}"), DVec3::new(f64::from(index) * 0.5, f64::from(index) + 0.5, 0.0)))
        .collect();
    let mut staircase = ChunkStructure::new(&staggered, Some(support)).unwrap();
    assert_eq!(ids(&staircase.commit().detached), ["step:3", "step:4", "step:5", "step:6"]);
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden recordings
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
                center: DVec3::new(
                    (f64::from(column) - half_columns) * width,
                    (f64::from(row) * height) + (height / 2.0),
                    0.0,
                ),
                size: DVec3::new(width, height, depth),
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
                    center: DVec3::new(
                        f64::from(side) * (half_columns * width),
                        (f64::from(row) * height) + (height / 2.0),
                        -f64::from(depth_index) * depth,
                    ),
                    size: DVec3::new(depth, height, depth),
                    material: "frame".to_owned(),
                    hit_points: 10.0,
                });
            }
        }
    }
    chunks
}

#[test]
fn form_destruction_wall_matches_the_recording_hit_for_hit() {
    let golden = golden();
    let mut case = golden.recording()["cases"][0].clone();
    assert_eq!(case["name"], "form-destruction");

    // The seeded generator's chunks are recorded too, so a change to the shared random stream shows in the recording.
    let generated = form_destruction_structure(case["seed"].as_i64().unwrap());
    case["definitions"] = to_record(&generated);
    let support: ChunkSupportOptions = serde_json::from_value(case["support"].clone()).unwrap();
    let mut structure = ChunkStructure::new(&generated, Some(support)).unwrap();

    // Hits replay their recorded aim, radius, and damage; everything else is produced.
    for step in case["steps"].as_array_mut().unwrap() {
        let kind = step["kind"].as_str().unwrap().to_owned();
        match kind.as_str() {
            "initial" => write_fields(step, state_record(&structure)),
            "hit" => {
                let ray = structure.raycast(vector(&step["origin"]), vector(&step["direction"]), AIM_DISTANCE).unwrap();
                step["ray"] = ray_record(ray.as_ref());
                if let Some(hit) = ray {
                    let amount = damage(hit.point, step["radius"].as_f64().unwrap(), step["damage"].as_f64().unwrap());
                    structure.queue_damage(amount).unwrap();
                }
                step["change"] = change_record(&structure.commit());
                step["state"] = state_record(&structure);
            }
            "mesh" => write_fields(step, mesh_summary_record(&structure)),
            "reset" => {
                step["change"] = change_record(&structure.reset());
                step["state"] = state_record(&structure);
            }
            other => panic!("unexpected step {other}"),
        }
    }
    golden.assert_section_matches("/cases/0", &case, &Tolerance::exact());
}

#[test]
fn framebreak_building_cascades_through_support_loss_and_the_collapse_rule() {
    let golden = golden();
    let mut case = golden.recording()["cases"][1].clone();
    assert_eq!(case["name"], "framebreak-tower");
    let definitions = definitions(&case["definitions"]);
    let support: ChunkSupportOptions = serde_json::from_value(case["support"].clone()).unwrap();
    assert_eq!(support.contact_tolerance, None, "the default contact tolerance applies");
    let mut structure = ChunkStructure::new(&definitions, Some(support)).unwrap();
    let initial_count = definitions.len() as f64;
    let mut collapsed = false;

    // Blasts replay their recorded center, radius, and damage; everything else is produced.
    for step in case["steps"].as_array_mut().unwrap() {
        if step["kind"] == "initial" {
            write_fields(step, state_record(&structure));
            continue;
        }
        let blast = &step["blast"];
        let blast =
            damage(vector(&blast["center"]), blast["radius"].as_f64().unwrap(), blast["damage"].as_f64().unwrap());
        structure.queue_damage(blast).unwrap();
        let first = structure.commit();
        step["change"] = change_record(&first);

        // The game's structural-failure rule runs a second commit at the same boundary; steps where it does not fire
        // record no collapse.
        let integrity = first.active_chunk_count as f64 / initial_count;
        if !collapsed && first.active_chunk_count > 0 && integrity < COLLAPSE_INTEGRITY {
            collapsed = true;
            let anchors: Vec<DestructibleChunkDefinition> = structure
                .active_chunks()
                .filter(|chunk| chunk.center[1] - (chunk.size[1] / 2.0) <= 0.001)
                .cloned()
                .collect();
            step["collapseAnchors"] = json!(ids(&anchors));
            for anchor in &anchors {
                structure.queue_damage(damage(anchor.center, 0.01, anchor.hit_points * 100.0)).unwrap();
            }
            step["collapse"] = change_record(&structure.commit());
        } else {
            let fields = step.as_object_mut().unwrap();
            fields.shift_remove("collapseAnchors");
            fields.shift_remove("collapse");
        }
        step["state"] = state_record(&structure);

        let occupancy: Vec<bool> =
            [DVec3::new(40.0, 1.0, -12.0), DVec3::new(36.25, 1.0, -14.5), DVec3::new(46.25, 14.4, -12.0)]
                .into_iter()
                .map(|center| structure.can_occupy(center, DVec3::new(0.6, 0.9, 0.6)).unwrap())
                .collect();
        step["occupancy"] = json!(occupancy);
    }
    assert!(collapsed);
    golden.assert_section_matches("/cases/1", &case, &Tolerance::exact());
}

#[test]
fn contact_tolerance_and_corner_rules_match_the_recorded_support() {
    let golden = golden();
    let mut case = golden.recording()["cases"][2].clone();
    assert_eq!(case["name"], "support-contacts");
    let support: ChunkSupportOptions = serde_json::from_value(case["support"].clone()).unwrap();
    assert_ne!(support.contact_tolerance, Some(DEFAULT_CONTACT_TOLERANCE));
    let mut structure = ChunkStructure::new(&definitions(&case["definitions"]), Some(support)).unwrap();

    // One initial state, then one commit that releases the unsupported chunks.
    let steps = case["steps"].as_array_mut().unwrap();
    write_fields(&mut steps[0], state_record(&structure));
    steps[1]["change"] = change_record(&structure.commit());
    steps[1]["state"] = state_record(&structure);
    golden.assert_section_matches("/cases/2", &case, &Tolerance::exact());
}

#[test]
fn chunk_mesh_data_and_removal_batches_match_the_recording() {
    let golden = golden();
    let recording = golden.recording();
    let wall_definitions = definitions(&recording["wall"]["definitions"]);
    let mesh = create_chunk_mesh_data(&wall_definitions);
    golden.assert_section_matches("/wall/mesh", &to_record(&mesh), &Tolerance::exact());

    let removed = vec!["wall:1:0".to_owned(), "wall:2:0".to_owned()];
    let batch = create_removal_mutation_batch(4, &removed).unwrap();
    golden.assert_section_matches("/batch", &to_record(&batch), &Tolerance::exact());
    let empty_batch = create_removal_mutation_batch(0, &[]).unwrap();
    golden.assert_section_matches("/emptyBatch", &to_record(&empty_batch), &Tolerance::exact());
    let error = create_removal_mutation_batch(1, &["ok".to_owned(), String::new()]).unwrap_err();
    golden.assert_section_matches("/batchError", &json!(error.to_string()), &Tolerance::exact());
}

#[test]
fn invalid_definitions_damage_and_queries_report_the_recorded_messages() {
    let message = |error: DestructionError| error.to_string();
    let chunks = wall();
    let mut invalid = chunks[0].clone();

    let duplicate = ChunkStructure::new(&[chunks[0].clone(), chunks[0].clone()], None).unwrap_err();
    invalid.material.clear();
    let identity = ChunkStructure::new(&[invalid], None).unwrap_err();
    let mut flat = chunks[0].clone();
    flat.size[1] = 0.0;
    let dimensions = ChunkStructure::new(&[flat], None).unwrap_err();
    let support = ChunkSupportOptions {
        ground_height: 0.0,
        maximum_cantilever_distance: -1.0,
        contact_tolerance: None,
    };
    let support = ChunkStructure::new(&chunks, Some(support)).unwrap_err();

    let mut structure = ChunkStructure::new(&chunks, None).unwrap();
    let damage_center = structure.queue_damage(damage(DVec3::new(0.0, f64::NAN, 0.0), 1.0, 1.0)).unwrap_err();
    let damage_amount = structure.queue_damage(damage(DVec3::splat(0.0), 0.0, 1.0)).unwrap_err();
    let raycast = structure.raycast(DVec3::splat(0.0), DVec3::new(0.0, 0.0, 1.0), 0.0).unwrap_err();
    assert_eq!(
        structure.raycast(DVec3::new(f64::NAN, 0.0, 0.0), DVec3::new(0.0, 0.0, 1.0), 1.0),
        Err(DestructionError::InvalidRaycast)
    );
    let occupancy = structure.can_occupy(DVec3::splat(0.0), DVec3::new(1.0, 0.0, 1.0)).unwrap_err();
    assert_eq!(structure.commit().revision, 0, "rejected damage never reaches the structure");

    // The messages are stable diagnostics, recorded by the kind of input they reject.
    let errors = json!({
        "duplicate": message(duplicate),
        "identity": message(identity),
        "dimensions": message(dimensions),
        "support": message(support),
        "damageCenter": message(damage_center),
        "damageAmount": message(damage_amount),
        "raycast": message(raycast),
        "occupancy": message(occupancy),
    });
    golden().assert_section_matches("/errors", &errors, &Tolerance::exact());
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
    let hit = physics.query(DVec3::new(0.0, 0.5, 4.0), DVec3::new(0.0, 0.0, -1.0), 10.0).unwrap();
    assert_eq!(hit.shape_id, "wall:2:0");
    assert!((hit.distance - 3.75).abs() < 1e-5 && (hit.point[2] - 0.25).abs() < 1e-5);

    // A committed removal becomes a batch at the same boundary; the query then passes through the hole.
    structure.queue_damage(damage(DVec3::new(0.0, 0.5, 0.0), 0.4, 2.0)).unwrap();
    let change = structure.commit();
    let removed: Vec<String> = change.removed.iter().map(|chunk| chunk.id.clone()).collect();
    physics.apply_mutations(&create_removal_mutation_batch(change.revision, &removed).unwrap()).unwrap();
    assert!(physics.query(DVec3::new(0.0, 0.5, 4.0), DVec3::new(0.0, 0.0, -1.0), 10.0).is_none());
    assert_eq!(physics.resident_shape_ids().count(), 14);

    // Stale, duplicate, and malformed batches change nothing.
    let stale = create_removal_mutation_batch(0, &["wall:0:0".to_owned()]).unwrap();
    assert_eq!(
        physics.apply_mutations(&stale),
        Err(PhysicsError::StaleMutationBatch {
            sequence: 0,
            last: 1
        })
    );
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
    let malformed = PhysicsMutationBatch {
        added_shapes: vec![flat],
        ..duplicate
    };
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
