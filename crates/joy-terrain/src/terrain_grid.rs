// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! One indexed regular lattice shared by every patch and every supported LOD.

/// A contiguous index range for one power-of-two stride. `first` is an index offset, not a byte offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainIndexRange {
    /// First index of the range.
    pub first: u32,
    /// Number of indices (three per triangle).
    pub count: u32,
}

/// The finest patch lattice and one index range per stride (`ranges[level]` draws stride `2^level`).
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainGrid {
    /// Patch-local sample offsets `(x, z)`, row-major, `(cells + 1)^2` long.
    pub positions: Vec<[f32; 2]>,
    /// Triangle indices for every stride, back to back.
    pub indices: Vec<u32>,
    /// Index ranges ordered from the finest stride (1) to the coarsest (`cells`).
    pub ranges: Vec<TerrainIndexRange>,
}

/// Build the shared lattice for a patch of `cells` cells per side (a power of two).
///
/// Coarser levels reference subsets of the finest vertices, so a topology change needs only a different index range.
/// Each cell splits along its top-left to bottom-right diagonal, matching height sampling and offline error measurement.
pub fn create_terrain_grid(cells: u32) -> TerrainGrid {
    let row = cells + 1;
    let mut positions = Vec::with_capacity((row * row) as usize);
    for z in 0..=cells {
        for x in 0..=cells {
            positions.push([x as f32, z as f32]);
        }
    }

    let mut indices = Vec::new();
    let mut ranges = Vec::new();
    let mut stride = 1;
    while stride <= cells {
        let first = indices.len() as u32;
        for z in (0..cells).step_by(stride as usize) {
            for x in (0..cells).step_by(stride as usize) {
                let a = (z * row) + x;
                let b = a + stride;
                let c = a + (stride * row);
                let d = c + stride;

                // A different diagonal would change the rendered surface.
                indices.extend_from_slice(&[a, d, b, a, c, d]);
            }
        }
        ranges.push(TerrainIndexRange {
            first,
            count: indices.len() as u32 - first,
        });
        stride *= 2;
    }
    TerrainGrid {
        positions,
        indices,
        ranges,
    }
}
