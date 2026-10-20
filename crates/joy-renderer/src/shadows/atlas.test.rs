// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/shadows/atlas.rs`.

use super::*;

fn overlaps(left: AtlasTile, right: AtlasTile) -> bool {
    left.x < right.x + right.size
        && right.x < left.x + left.size
        && left.y < right.y + right.size
        && right.y < left.y + left.size
}

#[test]
fn crowded_lights_shrink_least_important_first_then_drop() {
    // Twenty-seven lights asking for 256-texel faces need 10.6 million texels; a 2048 atlas holds 4.2 million.
    let mut sizes = vec![256; 27];
    fit_face_sizes(&mut sizes, 2048, 32);
    assert_eq!(sizes.len(), 27, "everything fits once faces shrink");
    assert!(sizes.windows(2).all(|pair| pair[0] >= pair[1]), "the most important lights keep the largest faces");
    let area: u64 = sizes.iter().map(|size| 6 * u64::from(*size).pow(2)).sum();
    assert!(area <= 2048 * 2048);

    // More lights than fit at the smallest face lose their shadows from the end.
    let mut sizes = vec![32; 4000];
    fit_face_sizes(&mut sizes, 1024, 32);
    assert_eq!(sizes.len(), (1024 * 1024) / (6 * 32 * 32));
}

#[test]
fn placed_faces_stay_inside_the_atlas_without_overlapping() {
    let mut sizes = vec![512, 256, 256, 128, 64, 64, 32, 256, 128];
    fit_face_sizes(&mut sizes, 2048, 32);
    let placed = place_faces(&sizes, 2048);
    let tiles: Vec<AtlasTile> = placed.iter().flatten().copied().collect();
    assert_eq!(tiles.len(), sizes.len() * 6);
    for (light, faces) in placed.iter().enumerate() {
        assert!(
            faces
                .iter()
                .all(|tile| tile.size == sizes[light] && tile.x + tile.size <= 2048 && tile.y + tile.size <= 2048)
        );
    }
    for (index, tile) in tiles.iter().enumerate() {
        assert!(tiles[index + 1..].iter().all(|other| !overlaps(*tile, *other)), "{tile:?} overlaps");
    }
}
