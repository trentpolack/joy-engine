// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Point-light shadow atlas layout: how large each light's faces may be, and where each face goes.
//!
//! Faces are power-of-two squares and the atlas is a power-of-two square, so placing them largest first by repeatedly
//! quartering free squares (a buddy allocator) always succeeds when their total area fits.

use crate::shadows::POINT_FACES;

/// One face's square in the atlas, in texels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AtlasTile {
    pub x: u32,
    pub y: u32,
    pub size: u32,
}

/// Shrink face sizes until six faces per light fit an `atlas`-texel square: each round halves every light larger than
/// `minimum` once, least important (last) first, and stops as soon as everything fits. Lights that still do not fit at
/// `minimum` are removed from the end. Sizes must be powers of two.
pub(crate) fn fit_face_sizes(sizes: &mut Vec<u32>, atlas: u32, minimum: u32) {
    let capacity = u64::from(atlas) * u64::from(atlas);
    let area = |sizes: &[u32]| sizes.iter().map(|size| POINT_FACES as u64 * u64::from(*size).pow(2)).sum::<u64>();
    while area(sizes) > capacity {
        let mut shrunk = false;
        for index in (0..sizes.len()).rev() {
            if sizes[index] > minimum {
                sizes[index] /= 2;
                shrunk = true;
                if area(sizes) <= capacity {
                    return;
                }
            }
        }
        if !shrunk {
            sizes.pop();
        }
    }
}

/// Place six faces of each light's size in an `atlas`-texel square; `sizes` must fit (see [`fit_face_sizes`]). Returns
/// each light's faces in face order.
pub(crate) fn place_faces(sizes: &[u32], atlas: u32) -> Vec<[AtlasTile; POINT_FACES]> {
    // Largest faces first; equal sizes keep light order.
    let mut order: Vec<(usize, usize)> =
        (0..sizes.len()).flat_map(|light| (0..POINT_FACES).map(move |face| (light, face))).collect();
    order.sort_by(|left, right| sizes[right.0].cmp(&sizes[left.0]));

    let mut free = vec![AtlasTile {
        x: 0,
        y: 0,
        size: atlas,
    }];
    let mut placed = vec![[AtlasTile::default(); POINT_FACES]; sizes.len()];
    for (light, face) in order {
        let size = sizes[light];
        // The smallest free square that holds the face, quartered down to its size.
        let Some(index) =
            (0..free.len()).filter(|index| free[*index].size >= size).min_by_key(|index| free[*index].size)
        else {
            continue;
        };
        let mut square = free.swap_remove(index);
        while square.size > size {
            let half = square.size / 2;
            free.push(AtlasTile {
                x: square.x + half,
                y: square.y,
                size: half,
            });
            free.push(AtlasTile {
                x: square.x,
                y: square.y + half,
                size: half,
            });
            free.push(AtlasTile {
                x: square.x + half,
                y: square.y + half,
                size: half,
            });
            square.size = half;
        }
        placed[light][face] = square;
    }
    placed
}

#[cfg(test)]
#[path = "atlas.test.rs"]
mod tests;
