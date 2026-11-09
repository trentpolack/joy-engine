// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Point-light shadow atlas layout: how large each light's faces may be, and where each face goes.
//!
//! Faces are power-of-two squares and the atlas is a power-of-two square, so placing them largest first by repeatedly
//! quartering free squares (a buddy allocator) always succeeds when their total area fits. The allocator is
//! persistent: a light keeps its tiles across frames while its shadow is cached, and released tiles merge with their
//! buddies again. Allocating into a fragmented atlas can fail even when the area fits; the shadow cache then repacks
//! every light largest first.

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

/// A buddy allocator of power-of-two square tiles in a power-of-two atlas.
#[derive(Clone, Debug)]
pub(crate) struct AtlasAllocator {
    size: u32,
    /// Free squares; no two are buddies of one free parent (they merge on release).
    free: Vec<AtlasTile>,
}

impl AtlasAllocator {
    /// An empty `size`-texel atlas.
    pub(crate) fn new(size: u32) -> Self {
        Self {
            size,
            free: vec![AtlasTile {
                x: 0,
                y: 0,
                size,
            }],
        }
    }

    /// The atlas size in texels.
    pub(crate) fn size(&self) -> u32 {
        self.size
    }

    /// Release every tile.
    pub(crate) fn reset(&mut self, size: u32) {
        *self = Self::new(size);
    }

    /// Place six faces of each light's size, largest first (equal sizes keep light order), into this allocator; returns
    /// each light's faces, or `None` (with whatever was placed left allocated) when one does not fit.
    pub(crate) fn place_all(&mut self, sizes: &[u32]) -> Option<Vec<[AtlasTile; POINT_FACES]>> {
        let mut order: Vec<(usize, usize)> =
            (0..sizes.len()).flat_map(|light| (0..POINT_FACES).map(move |face| (light, face))).collect();
        order.sort_by(|left, right| sizes[right.0].cmp(&sizes[left.0]));
        let mut placed = vec![[AtlasTile::default(); POINT_FACES]; sizes.len()];
        for (light, face) in order {
            placed[light][face] = self.allocate(sizes[light])?;
        }
        Some(placed)
    }

    /// Take a `size`-texel square (a power of two): the smallest free square that holds it, quartered down to its
    /// size. `None` when no free square is large enough.
    pub(crate) fn allocate(&mut self, size: u32) -> Option<AtlasTile> {
        let index = (0..self.free.len()).filter(|index| self.free[*index].size >= size).min_by_key(|index| {
            let tile = self.free[*index];
            (tile.size, tile.y, tile.x)
        })?;
        let mut square = self.free.swap_remove(index);
        while square.size > size {
            let half = square.size / 2;
            self.free.push(AtlasTile {
                x: square.x + half,
                y: square.y,
                size: half,
            });
            self.free.push(AtlasTile {
                x: square.x,
                y: square.y + half,
                size: half,
            });
            self.free.push(AtlasTile {
                x: square.x + half,
                y: square.y + half,
                size: half,
            });
            square.size = half;
        }
        Some(square)
    }

    /// Return a tile, merging it with its three buddies into their parent square whenever all four are free.
    pub(crate) fn release(&mut self, tile: AtlasTile) {
        let mut square = tile;
        while square.size < self.size {
            let parent_size = square.size * 2;
            let parent = AtlasTile {
                x: square.x - (square.x % parent_size),
                y: square.y - (square.y % parent_size),
                size: parent_size,
            };
            let quarters =
                [(0, 0), (square.size, 0), (0, square.size), (square.size, square.size)].map(|(x, y)| AtlasTile {
                    x: parent.x + x,
                    y: parent.y + y,
                    size: square.size,
                });
            let buddies_free =
                quarters.iter().filter(|quarter| **quarter != square).all(|quarter| self.free.contains(quarter));
            if !buddies_free {
                break;
            }
            self.free.retain(|free| !quarters.contains(free));
            square = parent;
        }
        self.free.push(square);
    }
}

#[cfg(test)]
#[path = "atlas.test.rs"]
mod tests;
