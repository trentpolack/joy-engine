// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Structure presentation from committed engine state; it never changes health or collision dimensions. The material
//! view uses the engine's chunk tints. The debug views inset faces slightly to reveal chunk boundaries, then color
//! remaining resistance or committed support paths.

use std::collections::HashMap;

use joy_engine::core::LinearColor;
use joy_engine::physics::{
    ChunkStructure,
    DestructibleChunkDefinition,
    create_chunk_mesh_data,
};
use joy_engine::render_types::Mesh;

use super::config::destruction_config::{
    DESTRUCTION,
    INTEGRITY,
    SUPPORT,
    StructureView,
};

/// Mesh data for the structure's active chunks in `view`.
pub fn create_structure_mesh_data(structure: &ChunkStructure, view: StructureView) -> Mesh {
    if view == StructureView::Material {
        return create_chunk_mesh_data(structure.active_chunks());
    }
    let health: HashMap<String, f64> =
        structure.damage_state().into_iter().map(|state| (state.chunk_id, state.remaining_hit_points)).collect();
    let support: HashMap<String, _> =
        structure.support_state().into_iter().map(|state| (state.chunk_id.clone(), state)).collect();
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    let mut colors = Vec::new();

    for chunk in structure.active_chunks() {
        let inset = DestructibleChunkDefinition {
            size: chunk.size.map(|size| size * INTEGRITY.mesh_inset),
            ..chunk.clone()
        };
        let geometry = create_chunk_mesh_data([&inset]);
        let vertex_offset = positions.len() as u32;
        let mut color = integrity_color(health.get(&chunk.id).copied().unwrap_or(chunk.hit_points));
        if view == StructureView::Support
            && let Some(state) = support.get(&chunk.id)
        {
            color = if state.anchored { SUPPORT.anchored_color } else { SUPPORT.vertical_color };
            if state.lateral_distance.unwrap_or(0.0) > 0.0 {
                color = SUPPORT.braced_color;
            }
        }
        positions.extend_from_slice(&geometry.positions);
        indices.extend(geometry.indices.iter().map(|index| vertex_offset + index));
        let tint = LinearColor::rgb(color[0] as f32, color[1] as f32, color[2] as f32);
        colors.extend(std::iter::repeat_n(tint, geometry.positions.len()));
    }
    Mesh {
        colors: Some(colors),
        ..Mesh::new(positions, indices)
    }
}

/// Resistance color on one absolute scale, so stronger frame chunks stay distinct from ordinary masonry.
pub fn integrity_color(remaining: f64) -> [f64; 3] {
    let middle = DESTRUCTION.hit_points / 2.0;
    let (first, second, fraction) = if remaining > DESTRUCTION.hit_points {
        let span = DESTRUCTION.frame_hit_points - DESTRUCTION.hit_points;
        (INTEGRITY.intact_color, INTEGRITY.reinforced_color, (remaining - DESTRUCTION.hit_points) / span)
    } else if remaining > middle {
        (INTEGRITY.weakened_color, INTEGRITY.intact_color, (remaining - middle) / middle)
    } else {
        (INTEGRITY.critical_color, INTEGRITY.weakened_color, remaining / middle)
    };
    let blend = fraction.clamp(0.0, 1.0);
    [0, 1, 2].map(|axis| first[axis] + ((second[axis] - first[axis]) * blend))
}
