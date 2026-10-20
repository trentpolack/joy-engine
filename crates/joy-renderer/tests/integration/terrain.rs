// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Terrain assets, LOD selection, and the morphed reference surface, plus selection and sampling on the Dragon Pit
//! example asset checked against the golden recording `tests/fixtures/terrain-reference.json` (four camera views,
//! collision samples, and morphed heights). Re-record with `JOY_UPDATE_FIXTURES=1` when terrain selection or sampling
//! changes on purpose.

use joy_core::math::DVec3;
use joy_core::math::deterministic;
use joy_renderer::terrain::{
    TerrainAsset,
    TerrainError,
    TerrainManifest,
    TerrainPatch,
    TerrainSelection,
    TerrainView,
    create_terrain_grid,
    decode_terrain,
    parse_terrain,
    reconcile_terrain_edges,
    sample_morphed_height,
    sample_terrain,
    select_terrain,
};
use joy_test_support::{
    Tolerance,
    golden_file,
};
use serde_json::{
    Value,
    json,
};

const DRAGON_PIT_MANIFEST: &str = include_str!("../../../../examples/terrain/assets/dragon-pit/terrain.json");
const DRAGON_PIT_HEIGHTS: &[u8] = include_bytes!("../../../../examples/terrain/assets/dragon-pit/height.bin");

/// Agreement required for LOD details, edges, and morphed heights: some recorded values predate the portable libm
/// functions and differ from them in the last bit. Collision samples must match exactly.
const SELECTION_TOLERANCE: f64 = 1e-12;

/// The Dragon Pit overview focus sits this far above the terrain at the world origin, in world units.
const FOCUS_HEIGHT_OFFSET: f64 = 12.0;

/// Leading patches of the near-ground view whose full descriptors are recorded.
const NEAR_GROUND_RECORDED_PATCHES: usize = 40;

/// The synthetic test terrain: 17 x 17 samples of `x^2 + z^2`, two by two 8-cell patches, one world unit per sample.
fn fixture_manifest() -> (TerrainManifest, Vec<u8>) {
    let size = 17;
    let mut binary = Vec::new();
    for z in 0..size {
        for x in 0..size {
            binary.extend_from_slice(&((x * x + z * z) as u16).to_le_bytes());
        }
    }
    let patches = (0..4)
        .map(|index| TerrainPatch {
            x: (index % 2) * 8,
            z: (index / 2) * 8,
            min: 0.0,
            max: 512.0,
            errors: vec![0.0, 2.0, 8.0, 32.0],
        })
        .collect();
    let manifest = TerrainManifest {
        version: 1,
        size,
        tile_cells: 8,
        extent: vec![16.0, 16.0],
        height_offset: 0.0,
        height_scale: 65535.0,
        height_file: "height.bin".into(),
        orientation: "row-0-to-negative-z".into(),
        patches,
    };
    (manifest, binary)
}

fn fixture() -> TerrainAsset {
    let (manifest, binary) = fixture_manifest();
    decode_terrain(manifest, &binary).expect("the fixture is valid")
}

fn view(eye: [f64; 3], target: [f64; 3], target_radius: f64) -> TerrainView {
    TerrainView {
        eye: DVec3::from_array(eye),
        projection_scale: 1000.0,
        tolerance: 2.0,
        target: DVec3::from_array(target),
        target_radius,
    }
}

fn morphed(asset: &TerrainAsset, patch: usize, x: f64, z: f64, selection: &TerrainSelection) -> f64 {
    sample_morphed_height(asset, patch, x, z, selection).expect("patch exists")
}

#[test]
fn assets_validate_payloads_and_sample_the_collision_triangles() {
    let asset = fixture();
    assert_eq!(sample_terrain(&asset, -8.0, -8.0), 0.0);
    assert_eq!(sample_terrain(&asset, 8.0, 8.0), 512.0);
    assert_eq!(sample_terrain(&asset, -7.5, -8.0), 0.5);

    let (mut manifest, binary) = fixture_manifest();
    manifest.extent = vec![0.0, 16.0];
    assert!(matches!(decode_terrain(manifest, &binary), Err(TerrainError::InvalidDimensions)));
    let (manifest, _) = fixture_manifest();
    assert!(matches!(decode_terrain(manifest, &[0, 0]), Err(TerrainError::InvalidDimensions)));
    let (mut manifest, _) = fixture_manifest();
    manifest.size = 25;
    assert!(matches!(decode_terrain(manifest, &vec![0; 25 * 25 * 2]), Err(TerrainError::InvalidDimensions)));
    let (mut manifest, binary) = fixture_manifest();
    manifest.patches[3].errors = vec![0.0, 8.0, 2.0, 32.0];
    assert!(matches!(decode_terrain(manifest, &binary), Err(TerrainError::InvalidPatch)));
    let (mut manifest, binary) = fixture_manifest();
    manifest.patches.pop();
    assert!(matches!(decode_terrain(manifest, &binary), Err(TerrainError::InvalidPatchCount)));
}

#[test]
fn selection_is_coarse_at_distance_and_finest_near_a_stable_target() {
    let asset = fixture();
    let far = select_terrain(&asset, &view([100000.0; 3], [0.0; 3], 0.0)).unwrap();
    assert_eq!(far.len(), 4);
    assert!(far.iter().all(|patch| patch.lod == 3));
    let close = select_terrain(&asset, &view([0.0, 600.0, 0.0], [-4.0, 0.0, -4.0], 1.0)).unwrap();
    assert_eq!(close[0].detail, 0.0);
}

#[test]
fn shared_edges_agree_across_arbitrary_neighboring_levels() {
    let asset = fixture();
    for detail in [0.0, 0.3, 0.99999, 1.0, 1.8, 2.99999, 3.0] {
        let patches = reconcile_terrain_edges(&asset, &[detail, 2.6, 0.2, 1.2]).unwrap();
        for z in 0..=8 {
            let left = morphed(&asset, 0, 8.0, f64::from(z), &patches[0]);
            let right = morphed(&asset, 1, 0.0, f64::from(z), &patches[1]);
            assert!((left - right).abs() < 1e-6, "seam at detail {detail}, z {z}: {left} != {right}");
        }
        let before = reconcile_terrain_edges(&asset, &[0.999999, 0.0, 0.0, 0.0]).unwrap()[0];
        let after = reconcile_terrain_edges(&asset, &[1.0, 0.0, 0.0, 0.0]).unwrap()[0];
        assert!((morphed(&asset, 0, 3.0, 3.0, &before) - morphed(&asset, 0, 3.0, 3.0, &after)).abs() < 1e-4);
    }
    assert!(matches!(reconcile_terrain_edges(&asset, &[0.0]), Err(TerrainError::InvalidPatchCount)));
}

#[test]
fn indexed_grids_share_vertices_with_valid_ranges_at_every_resolution() {
    let grid = create_terrain_grid(8);
    assert_eq!(grid.positions.len(), 81);
    assert_eq!(grid.ranges.len(), 4);
    assert_eq!(grid.ranges[0].count, 8 * 8 * 6);
    assert_eq!(grid.ranges[3].count, 6);
    assert!(grid.indices.iter().all(|index| *index < 81));
}

#[test]
fn target_detail_lock_fades_continuously() {
    let asset = fixture();
    let base = view([10000.0; 3], [-4.0, 0.0, -4.0], 1.0);
    let inside = select_terrain(
        &asset,
        &TerrainView {
            target: DVec3::new(1.0 - 1e-6, 0.0, -4.0),
            ..base
        },
    )
    .unwrap();
    let outside = select_terrain(
        &asset,
        &TerrainView {
            target: DVec3::new(1.0 + 1e-6, 0.0, -4.0),
            ..base
        },
    )
    .unwrap();
    assert!((inside[0].detail - outside[0].detail).abs() < 0.001);
    let beyond = select_terrain(
        &asset,
        &TerrainView {
            target: DVec3::new(1.5, 0.0, -4.0),
            ..base
        },
    )
    .unwrap();
    assert!(beyond[0].detail > 0.0);
}

#[test]
fn invalid_views_fail_before_selection() {
    let asset = fixture();
    let base = TerrainView {
        projection_scale: 100.0,
        ..view([0.0, 10.0, 0.0], [0.0; 3], 1.0)
    };
    for invalid in [
        TerrainView {
            target_radius: f64::NAN,
            ..base
        },
        TerrainView {
            target_radius: -1.0,
            ..base
        },
        TerrainView {
            eye: DVec3::new(0.0, f64::INFINITY, 0.0),
            ..base
        },
        TerrainView {
            projection_scale: 0.0,
            ..base
        },
        TerrainView {
            tolerance: 0.0,
            ..base
        },
    ] {
        assert!(matches!(select_terrain(&asset, &invalid), Err(TerrainError::InvalidView)));
    }
}

#[test]
fn removed_vertices_converge_to_stitched_parent_triangles_beside_coarser_edges() {
    let asset = fixture();
    for level in [1.0_f64, 2.0, 3.0] {
        for neighbor in [level, (level + 0.6).min(3.0), 3.0] {
            let before = reconcile_terrain_edges(&asset, &[neighbor, level - 1e-7, 3.0, 3.0]).unwrap()[1];
            let after = reconcile_terrain_edges(&asset, &[neighbor, level, 3.0, 3.0]).unwrap()[1];
            let stride = deterministic::exp2(level);
            for z in 1..8 {
                for x in 1..8 {
                    let (x, z) = (f64::from(x), f64::from(z));
                    let base_x = (x / stride).floor() * stride;
                    let base_z = (z / stride).floor() * stride;
                    let (fraction_x, fraction_z) = ((x - base_x) / stride, (z - base_z) / stride);
                    let a = morphed(&asset, 1, base_x, base_z, &after);
                    let b = morphed(&asset, 1, base_x + stride, base_z, &after);
                    let c = morphed(&asset, 1, base_x, base_z + stride, &after);
                    let d = morphed(&asset, 1, base_x + stride, base_z + stride, &after);
                    let plane = if fraction_x >= fraction_z {
                        a + (b - a) * fraction_x + (d - b) * fraction_z
                    } else {
                        a + (d - c) * fraction_x + (c - a) * fraction_z
                    };
                    let height = morphed(&asset, 1, x, z, &before);
                    assert!((height - plane).abs() < 1e-4, "level {level}, neighbor {neighbor}, vertex {x},{z}");
                }
            }
        }
    }
}

/// A recorded camera view: its eye, target, projection scale, tolerance, and stable-detail radius.
fn recorded_view(recorded: &Value) -> TerrainView {
    let number = |value: &Value| value.as_f64().expect("recorded numbers");
    let vector = |value: &Value| DVec3::new(number(&value[0]), number(&value[1]), number(&value[2]));
    TerrainView {
        eye: vector(&recorded["eye"]),
        projection_scale: number(&recorded["projectionScale"]),
        tolerance: number(&recorded["tolerance"]),
        target: vector(&recorded["target"]),
        target_radius: number(&recorded["targetRadius"]),
    }
}

/// One selected patch: fractional detail, drawn LOD, and reconciled edge details.
fn patch_record(patch: &TerrainSelection) -> Value {
    json!({"detail": patch.detail, "lod": patch.lod, "edges": patch.edges})
}

#[test]
fn dragon_pit_selection_and_sampling_match_the_recording() {
    let asset = parse_terrain(DRAGON_PIT_MANIFEST, DRAGON_PIT_HEIGHTS).expect("the example asset is valid");
    assert_eq!(asset.patches().len(), 256);
    assert_eq!(asset.max_lod(), 6);
    let golden = golden_file!("terrain-reference.json");
    let mut produced = golden.recording().clone();
    let coordinate = |sample: &Value, axis: &str| sample[axis].as_f64().expect("recorded coordinates");

    // The overview and near-ground views look at this focus.
    produced["focusHeight"] = json!(sample_terrain(&asset, 0.0, 0.0) + FOCUS_HEIGHT_OFFSET);

    // Views replay their recorded cameras; the per-patch details are produced.
    for view in produced["views"].as_array_mut().unwrap() {
        let selection = select_terrain(&asset, &recorded_view(view)).unwrap();
        view["details"] = json!(selection.iter().map(|patch| patch.detail).collect::<Vec<_>>());
    }

    // Collision samples replay their recorded positions.
    for sample in produced["samples"].as_array_mut().unwrap() {
        sample["height"] = json!(sample_terrain(&asset, coordinate(sample, "x"), coordinate(sample, "z")));
    }

    // Full near-ground descriptors (detail, LOD, and reconciled edges) and the morphed surface under them.
    let near_ground = produced["views"].as_array().unwrap().iter().find(|view| view["name"] == "near-ground").unwrap();
    let selection = select_terrain(&asset, &recorded_view(near_ground)).unwrap();
    produced["nearGroundSelection"] = selection[..NEAR_GROUND_RECORDED_PATCHES].iter().map(patch_record).collect();
    for sample in produced["morphed"].as_array_mut().unwrap() {
        let patch = sample["patch"].as_u64().unwrap() as usize;
        let height = morphed(&asset, patch, coordinate(sample, "x"), coordinate(sample, "z"), &selection[patch]);
        sample["height"] = json!(height);
    }

    golden.assert_matches(&produced, &Tolerance::relative(SELECTION_TOLERANCE).with_key("samples", 0.0));
}
