// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/editor_grid.rs`.

use super::*;

fn view() -> EditorGridView {
    EditorGridView {
        target: [0.0; 3],
        backward: [0.4, 0.7, 0.6],
        distance: 26.0,
        viewport_height_pixels: 800.0,
        height: 0.0,
    }
}

#[test]
fn grids_stay_bounded_and_translucent_across_scales() {
    let mut grid = EditorGrid::new();
    let first = grid.frame(&view()).unwrap().clone();
    assert!(!first.triangles.is_empty());
    let bound = first.triangles.len();
    for distance in [0.01, 1.0, 10.0, 100.0, 1000.0, 100000.0] {
        for backward in [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.3, -0.8, 0.5]] {
            let changed =
                EditorGridView { target: [1000.0, 4.0, -1000.0], backward, distance, height: -120.0, ..view() };
            let frame = grid.frame(&changed).unwrap();
            assert!(!frame.triangles.is_empty() && frame.triangles.len() <= bound, "zoom cannot grow payload");
            assert_eq!(frame.triangles.len() % 3, 0);
            let power = deterministic::powf(10_f64, deterministic::log10(frame.spacing).floor());
            assert!([1.0, 2.0, 5.0].iter().any(|value| (frame.spacing / power - value).abs() < 1e-10));
            assert!(frame.triangles.iter().all(|vertex| vertex.color[3] > 0.0 && vertex.color[3] < 1.0));
        }
    }
    // Independent owners reproduce the same world-unit grid.
    assert_eq!(EditorGrid::new().frame(&view()).unwrap(), &first);
}

#[test]
fn invalid_views_fail_without_replacing_the_cache() {
    let mut grid = EditorGrid::new();
    let first = grid.frame(&view()).unwrap().clone();
    let changes = [
        EditorGridView { distance: 0.0, ..view() },
        EditorGridView { viewport_height_pixels: -1.0, ..view() },
        EditorGridView { height: f64::NAN, ..view() },
        EditorGridView { target: [0.0, f64::INFINITY, 0.0], ..view() },
        EditorGridView { backward: [0.0; 3], ..view() },
        EditorGridView { distance: f64::MAX, ..view() },
        EditorGridView { target: [1e100, 0.0, 0.0], ..view() },
    ];
    for changed in changes {
        assert!(grid.frame(&changed).is_err(), "{changed:?}");
        assert_eq!(grid.frame(&view()).unwrap(), &first);
    }
}
