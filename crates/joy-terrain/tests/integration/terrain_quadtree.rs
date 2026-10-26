// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Quadtree selection: one-level pyramids select exactly like version 1 assets, neighbors of different levels share
//! every edge, and the drawn surface moves continuously as the camera approaches (no pops at subdivision).

use joy_core::math::deterministic;
use joy_terrain::{
    TerrainNode,
    TerrainSurface,
    TerrainView,
    coarsen_terrain_nodes,
    sample_node_height,
    select_terrain,
    select_terrain_nodes,
};
use serde_json::Value;

use crate::{
    dragon_pit,
    rolling_pyramid,
    view,
};

/// Agreement required between the quadtree and version 1 selections (they compute footprints in different orders).
const PARITY_TOLERANCE: f64 = 1e-9;
/// Agreement required between the two sides of an edge (`f64` math on `f32` samples).
const SEAM_TOLERANCE: f64 = 1e-4;
/// A low-resolution projection (CSS pixels per unit at unit distance) so the rough test terrain mixes levels.
const COARSE_PROJECTION_SCALE: f64 = 40.0;

#[test]
fn one_level_pyramids_select_like_version_one_assets() {
    let asset = dragon_pit();
    let surface = TerrainSurface::from_asset(&asset);
    let recording: Value =
        serde_json::from_str(include_str!("../fixtures/terrain-reference.json")).expect("the recording parses");
    for recorded in recording["views"].as_array().expect("recorded views") {
        let number = |value: &Value| value.as_f64().expect("recorded numbers");
        let vector = |value: &Value| [number(&value[0]), number(&value[1]), number(&value[2])];
        let camera = TerrainView {
            projection_scale: number(&recorded["projectionScale"]),
            tolerance: number(&recorded["tolerance"]),
            ..view(vector(&recorded["eye"]), vector(&recorded["target"]), number(&recorded["targetRadius"]))
        };
        let expected = select_terrain(&asset, &camera).expect("valid view");
        let selection = select_terrain_nodes(&surface, &camera).expect("valid view");
        assert_eq!(selection.nodes.len(), expected.len());
        assert!(selection.wanted.is_empty(), "a one-level pyramid has nothing to stream");
        for (index, (node, leaf)) in selection.nodes.iter().zip(&expected).enumerate() {
            let patch = &asset.patches()[index];
            assert_eq!((node.key.x * 64, node.key.z * 64), (patch.x, patch.z));
            assert!((node.detail - leaf.detail).abs() < PARITY_TOLERANCE, "patch {index} detail");
            assert_eq!(node.lod() as usize, leaf.lod);
            for (edge, reference) in node.edges.iter().zip(leaf.edges) {
                assert!((edge - reference).abs() < PARITY_TOLERANCE, "patch {index} edge");
            }
        }
    }
}

#[test]
fn neighbors_of_different_levels_share_every_edge() {
    let (layout, tiles) = rolling_pyramid(4);
    let surface = TerrainSurface::from_tiles(layout, tiles, Default::default()).expect("complete pyramid");
    let mut mixed_edges = 0;
    for (eye, target) in [
        ([-30.0, 12.0, -40.0], [-50.0, 0.0, -50.0]),
        ([10.0, 6.0, 5.0], [0.0, 0.0, 0.0]),
        ([60.0, 30.0, -10.0], [40.0, 0.0, 20.0]),
        ([0.0, 120.0, 0.0], [0.0, 0.0, 0.0]),
        ([-5.0, 3.0, 55.0], [-20.0, 0.0, 40.0]),
    ] {
        let camera = TerrainView {
            projection_scale: COARSE_PROJECTION_SCALE,
            ..view(eye, target, 0.0)
        };
        let selected = select_terrain_nodes(&surface, &camera).expect("valid view").nodes;

        // Coarsening some tiles (as shadow views do for casters the camera cannot see) keeps every edge shared.
        let coarsened = coarsen_terrain_nodes(&surface, &selected, |_, node| {
            if (node.key.x + node.key.z) % 2 == 0 { f64::from(node.key.level + 3) } else { node.detail }
        });
        assert!(coarsened.iter().zip(&selected).any(|(coarse, fine)| coarse.lod() > fine.lod()));
        for nodes in [selected, coarsened] {
            assert_seamless(&surface, &nodes, &mut mixed_edges);
        }
    }
    assert!(mixed_edges > 0, "the views should place tiles of different levels side by side");
}

/// Every pair of drawn nodes that share an edge differs by at most one level and draws the same heights along it.
fn assert_seamless(surface: &TerrainSurface, nodes: &[TerrainNode], mixed_edges: &mut usize) {
    for (first, a) in nodes.iter().enumerate() {
        for b in &nodes[first + 1..] {
            let Some(segment) = shared_segment(surface, a, b) else {
                continue;
            };
            assert!((a.key.level - b.key.level).abs() <= 1, "{:?} borders {:?}", a.key, b.key);
            *mixed_edges += usize::from(a.key.level != b.key.level);
            for (x, z) in segment {
                let left = rendered_height(surface, a, x, z);
                let right = rendered_height(surface, b, x, z);
                assert!((left - right).abs() < SEAM_TOLERANCE, "crack between {:?} and {:?} at {x},{z}", a.key, b.key);
            }
        }
    }
}

#[test]
fn the_drawn_surface_moves_continuously_as_the_camera_descends() {
    let (layout, tiles) = rolling_pyramid(4);
    let surface = TerrainSurface::from_tiles(layout, tiles, Default::default()).expect("complete pyramid");

    // A continuous surface moves about four times less per step when the steps are four times smaller; a pop (a
    // subdivision or LOD switch that changes the drawn surface at once) jumps by the same amount at any step size.
    let coarse = largest_descent_step(&surface, 0.02);
    let fine = largest_descent_step(&surface, 0.005);
    assert!(fine < coarse * 0.5, "the surface jumped {fine} units in a 0.5% step ({coarse} in a 2% step)");
}

/// The largest height change of the drawn surface at a grid of probes between consecutive camera positions, while the
/// camera descends from 400 to 4 units by `fraction` of its altitude per step.
fn largest_descent_step(surface: &TerrainSurface, fraction: f64) -> f64 {
    let probes: Vec<(f64, f64)> =
        (0..=16).flat_map(|z| (0..=16).map(move |x| (f64::from(x) * 8.0, f64::from(z) * 8.0))).collect();
    let mut previous: Option<Vec<f64>> = None;
    let mut altitude = 400.0;
    let mut largest: f64 = 0.0;
    while altitude > 4.0 {
        let camera = TerrainView {
            projection_scale: COARSE_PROJECTION_SCALE,
            ..view([-6.0, altitude, 9.0], [0.0, 0.0, 0.0], 0.0)
        };
        let nodes = select_terrain_nodes(surface, &camera).expect("valid view").nodes;
        let heights: Vec<f64> = probes
            .iter()
            .map(|&(x, z)| rendered_height(surface, containing_node(surface, &nodes, x, z), x, z))
            .collect();
        if let Some(before) = &previous {
            for (old, new) in before.iter().zip(&heights) {
                largest = largest.max((old - new).abs());
            }
        }
        previous = Some(heights);
        altitude *= 1.0 - fraction;
    }
    largest
}

/// Finest-lattice points along the boundary two nodes share, or `None` when they only touch at a corner or not at all.
fn shared_segment(surface: &TerrainSurface, a: &TerrainNode, b: &TerrainNode) -> Option<Vec<(f64, f64)>> {
    let layout = surface.layout();
    let footprint = |node: &TerrainNode| {
        let origin = layout.tile_origin_samples(node.key);
        let span = layout.tile_span_samples(node.key.level);
        (origin, [origin[0] + span, origin[1] + span])
    };
    let ((a_low, a_high), (b_low, b_high)) = (footprint(a), footprint(b));
    let overlap = |axis: usize| (a_low[axis].max(b_low[axis]), a_high[axis].min(b_high[axis]));
    let finest = deterministic::powi(2.0, surface.finest_level());
    let points = |fixed_axis: usize, line: f64, (start, end): (f64, f64)| {
        let count = ((end - start) / finest).round() as usize;
        (0..=count)
            .map(|step| {
                let along = start + (step as f64 * finest);
                if fixed_axis == 0 { (line, along) } else { (along, line) }
            })
            .collect::<Vec<_>>()
    };
    for axis in 0..2 {
        let other = 1 - axis;
        let range = overlap(other);
        if range.1 <= range.0 {
            continue;
        }
        if a_high[axis] == b_low[axis] {
            return Some(points(axis, a_high[axis], range));
        }
        if b_high[axis] == a_low[axis] {
            return Some(points(axis, a_low[axis], range));
        }
    }
    None
}

/// The finest drawn node containing a finest-lattice point.
fn containing_node<'a>(surface: &TerrainSurface, nodes: &'a [TerrainNode], x: f64, z: f64) -> &'a TerrainNode {
    let layout = surface.layout();
    nodes
        .iter()
        .filter(|node| {
            let origin = layout.tile_origin_samples(node.key);
            let span = layout.tile_span_samples(node.key.level);
            (origin[0]..=origin[0] + span).contains(&x) && (origin[1]..=origin[1] + span).contains(&z)
        })
        .min_by_key(|node| node.key.level)
        .expect("drawn nodes cover the terrain")
}

/// The height of a node's drawn mesh (its vertices at the drawn stride, split on the diagonal) at a finest-lattice
/// point inside its footprint.
fn rendered_height(surface: &TerrainSurface, node: &TerrainNode, x: f64, z: f64) -> f64 {
    let layout = surface.layout();
    let origin = layout.tile_origin_samples(node.key);
    let step = deterministic::powi(2.0, node.key.level);
    let cells = f64::from(layout.tile_cells);
    let (local_x, local_z) = ((x - origin[0]) / step, (z - origin[1]) / step);
    let stride = f64::from(1_u32 << node.lod());
    let base_x = ((local_x / stride).floor() * stride).min(cells - stride);
    let base_z = ((local_z / stride).floor() * stride).min(cells - stride);
    let (fx, fz) = ((local_x - base_x) / stride, (local_z - base_z) / stride);
    let vertex = |vx: f64, vz: f64| sample_node_height(surface, node, vx, vz).expect("drawn nodes are resident");
    let a = vertex(base_x, base_z);
    let b = vertex(base_x + stride, base_z);
    let c = vertex(base_x, base_z + stride);
    let d = vertex(base_x + stride, base_z + stride);
    if fx >= fz { a + (b - a) * fx + (d - b) * fz } else { a + (d - c) * fx + (c - a) * fz }
}
