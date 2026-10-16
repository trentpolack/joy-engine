// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Terrain assets, LOD selection, and the morphed reference surface: the TypeScript `test/terrain.test.mjs` scenarios,
//! plus exact parity with selection and sampling recorded from the TypeScript engine on the Dragon Pit example asset
//! (`test/fixtures/terrain-reference.json`, four camera views, collision samples, and morphed heights).

use glam::DVec3;
use joy_engine::rendering::terrain::{
    TerrainAsset, TerrainError, TerrainManifest, TerrainPatch, TerrainSelection, TerrainView, create_terrain_grid,
    decode_terrain, parse_terrain, reconcile_terrain_edges, sample_morphed_height, sample_terrain, select_terrain,
};
use serde_json::Value;

const REFERENCE: &str = include_str!("../fixtures/terrain-reference.json");
const DRAGON_PIT_MANIFEST: &str = include_str!("../../examples/terrain/site/public/dragon-pit/terrain.json");
const DRAGON_PIT_HEIGHTS: &[u8] = include_bytes!("../../examples/terrain/site/public/dragon-pit/height.bin");

/// The TypeScript fixture: 17 x 17 samples of `x^2 + z^2`, two by two 8-cell patches, one world unit per sample.
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
    let inside = select_terrain(&asset, &TerrainView { target: DVec3::new(1.0 - 1e-6, 0.0, -4.0), ..base }).unwrap();
    let outside = select_terrain(&asset, &TerrainView { target: DVec3::new(1.0 + 1e-6, 0.0, -4.0), ..base }).unwrap();
    assert!((inside[0].detail - outside[0].detail).abs() < 0.001);
    let beyond = select_terrain(&asset, &TerrainView { target: DVec3::new(1.5, 0.0, -4.0), ..base }).unwrap();
    assert!(beyond[0].detail > 0.0);
}

#[test]
fn invalid_views_fail_before_selection() {
    let asset = fixture();
    let base = TerrainView { projection_scale: 100.0, ..view([0.0, 10.0, 0.0], [0.0; 3], 1.0) };
    for invalid in [
        TerrainView { target_radius: f64::NAN, ..base },
        TerrainView { target_radius: -1.0, ..base },
        TerrainView { eye: DVec3::new(0.0, f64::INFINITY, 0.0), ..base },
        TerrainView { projection_scale: 0.0, ..base },
        TerrainView { tolerance: 0.0, ..base },
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
            let stride = level.exp2();
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

#[test]
fn dragon_pit_selection_and_sampling_match_typescript() {
    let asset = parse_terrain(DRAGON_PIT_MANIFEST, DRAGON_PIT_HEIGHTS).expect("the example asset is valid");
    assert_eq!(asset.patches().len(), 256);
    assert_eq!(asset.max_lod(), 6);
    let reference: Value = serde_json::from_str(REFERENCE).unwrap();
    let number = |value: &Value| value.as_f64().expect("recorded numbers");
    let vector = |value: &Value| DVec3::new(number(&value[0]), number(&value[1]), number(&value[2]));

    for recorded in reference["views"].as_array().unwrap() {
        let view = TerrainView {
            eye: vector(&recorded["eye"]),
            projection_scale: number(&recorded["projectionScale"]),
            tolerance: number(&recorded["tolerance"]),
            target: vector(&recorded["target"]),
            target_radius: number(&recorded["targetRadius"]),
        };
        let selection = select_terrain(&asset, &view).unwrap();
        let name = recorded["name"].as_str().unwrap();
        for (index, (patch, expected)) in selection.iter().zip(recorded["details"].as_array().unwrap()).enumerate() {
            assert_eq!(patch.detail, number(expected), "{name} view, patch {index}");
        }
    }

    for sample in reference["samples"].as_array().unwrap() {
        let height = sample_terrain(&asset, number(&sample["x"]), number(&sample["z"]));
        assert_eq!(height, number(&sample["height"]), "collision sample {sample}");
    }

    // Full near-ground descriptors (detail, LOD, and reconciled edges) and the morphed surface under them.
    let near = &reference["views"][1];
    let view = TerrainView {
        eye: vector(&near["eye"]),
        projection_scale: number(&near["projectionScale"]),
        tolerance: number(&near["tolerance"]),
        target: vector(&near["target"]),
        target_radius: number(&near["targetRadius"]),
    };
    let selection = select_terrain(&asset, &view).unwrap();
    for (index, expected) in reference["nearGroundSelection"].as_array().unwrap().iter().enumerate() {
        let patch = selection[index];
        assert_eq!(patch.detail, number(&expected["detail"]));
        assert_eq!(patch.lod as f64, number(&expected["lod"]));
        let edges: Vec<f64> = expected["edges"].as_array().unwrap().iter().map(number).collect();
        assert_eq!(patch.edges.to_vec(), edges, "patch {index} edges");
    }
    for sample in reference["morphed"].as_array().unwrap() {
        let patch = sample["patch"].as_u64().unwrap() as usize;
        let height = morphed(&asset, patch, number(&sample["x"]), number(&sample["z"]), &selection[patch]);
        assert_eq!(height, number(&sample["height"]), "morphed sample {sample}");
    }
}
