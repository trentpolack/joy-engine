// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use super::*;

#[test]
fn tiles_fill_the_grid_without_overlapping() {
    assert_eq!(BUFFER_OVERVIEW_TILES.len(), (BUFFER_OVERVIEW_COLUMNS * BUFFER_OVERVIEW_ROWS) as usize);
    let rects: Vec<[f32; 4]> =
        (0..BUFFER_OVERVIEW_TILES.len()).map(|index| buffer_overview_tile_rect(index, 1600.0, 900.0, 4.0)).collect();
    for (index, a) in rects.iter().enumerate() {
        assert!(a[0] + a[2] <= 1600.0 && a[1] + a[3] <= 900.0, "tile {index} stays inside the output");
        for b in &rects[index + 1..] {
            let separate = a[0] + a[2] <= b[0] || b[0] + b[2] <= a[0] || a[1] + a[3] <= b[1] || b[1] + b[3] <= a[1];
            assert!(separate, "tiles overlap: {a:?} {b:?}");
        }
    }
}
