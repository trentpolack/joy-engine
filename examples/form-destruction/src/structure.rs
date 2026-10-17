// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The seeded destructible structure (TypeScript `structure.ts`): a 13×6 front wall of masonry chunks and a shallow
//! U-shaped return shell of stronger frame chunks. Coordinate-derived ids stay stable; the seed only varies materials.

use joy_engine::core::{
    RandomSource,
    SeededRandom,
};
use joy_engine::physics::DestructibleChunkDefinition;

use super::config::destruction_config::DESTRUCTION;

/// Fraction of front-wall chunks drawn as interior material.
const INTERIOR_CHANCE: f64 = 0.22;
/// Return-shell rows and depth, in chunks.
const SHELL_ROWS: u32 = 4;
const SHELL_DEPTH: u32 = 6;

/// Generate the wall and return-shell chunks for `seed`, in the TypeScript generation order.
pub fn generate_structure(seed: i64) -> Vec<DestructibleChunkDefinition> {
    let mut random = SeededRandom::new(seed);
    let [width, height, depth] = DESTRUCTION.chunk_size;
    let half_columns = f64::from(DESTRUCTION.wall_columns - 1) / 2.0;
    let mut chunks = Vec::new();

    for row in 0..DESTRUCTION.wall_rows {
        for column in 0..DESTRUCTION.wall_columns {
            let material = if random.next_unit() > INTERIOR_CHANCE { "masonry" } else { "interior" };
            chunks.push(DestructibleChunkDefinition {
                id: format!("front:{column}:{row}"),
                center: [(f64::from(column) - half_columns) * width, (f64::from(row) * height) + (height / 2.0), 0.0],
                size: [width, height, depth],
                material: material.to_owned(),
                hit_points: DESTRUCTION.hit_points,
            });
        }
    }

    // A shallow U-shaped shell exposes return walls when inspecting a damaged front face.
    for row in 0..SHELL_ROWS {
        for depth_index in 1..=SHELL_DEPTH {
            for side in [-1, 1] {
                chunks.push(DestructibleChunkDefinition {
                    id: format!("side:{side}:{depth_index}:{row}"),
                    center: [
                        f64::from(side) * (half_columns * width),
                        (f64::from(row) * height) + (height / 2.0),
                        -f64::from(depth_index) * depth,
                    ],
                    size: [depth, height, depth],
                    material: "frame".to_owned(),
                    hit_points: DESTRUCTION.frame_hit_points,
                });
            }
        }
    }
    chunks
}
