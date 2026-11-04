// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Structure presentation from committed engine state; it never changes health, collision dimensions, or stress. The
//! material view uses the engine's chunk tints. The debug views inset faces slightly to reveal chunk boundaries, then
//! color remaining resistance, committed support paths, or load over capacity.
//!
//! Every view but SUPPORT telegraphs stress: strained chunks lean toward amber, and failing chunks pulse red and shake
//! harder as their failure delay runs out. The telegraph is cosmetic and reads presentation time only.

use std::collections::HashMap;
use std::f64::consts::TAU;

use joy_engine::core::LinearColor;
use joy_engine::math::deterministic;
use joy_engine::physics::{
    ChunkStressStage,
    ChunkStressState,
    ChunkStructure,
    DestructibleChunkDefinition,
    create_chunk_mesh_data,
};
use joy_engine::render_types::Mesh;

use super::config::destruction_config::{
    DESTRUCTION,
    INTEGRITY,
    STRESS,
    SUPPORT,
    StructureView,
};

/// Shake frequencies along X and Z, in radians per second; unrelated so the motion does not read as a circle.
const SHAKE_FREQUENCIES: [f64; 2] = [41.0, 29.0];

/// Mesh data for the structure's active chunks in `view`, with the stress telegraph at `time_seconds` of
/// presentation time.
pub fn create_structure_mesh_data(structure: &ChunkStructure, view: StructureView, time_seconds: f64) -> Mesh {
    let health: HashMap<String, f64> =
        structure.damage_state().into_iter().map(|state| (state.chunk_id, state.remaining_hit_points)).collect();
    let support: HashMap<String, _> =
        structure.support_state().into_iter().map(|state| (state.chunk_id.clone(), state)).collect();
    let stress: HashMap<&str, &ChunkStressState> =
        structure.stress_states().iter().map(|state| (state.chunk_id.as_str(), state)).collect();
    let inset = if view == StructureView::Material { 1.0 } else { INTEGRITY.mesh_inset };
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    let mut colors = Vec::new();

    for (chunk_index, chunk) in structure.active_chunks().enumerate() {
        let sized = DestructibleChunkDefinition {
            size: chunk.size.map(|size| size * inset),
            ..chunk.clone()
        };
        let geometry = create_chunk_mesh_data([&sized]);

        // Base color for the view.
        let mut color = match view {
            StructureView::Material => {
                let tint = geometry.colors.as_ref().and_then(|colors| colors.first()).copied();
                tint.map_or([1.0; 3], |tint| [f64::from(tint.red), f64::from(tint.green), f64::from(tint.blue)])
            }
            StructureView::Integrity => integrity_color(health.get(&chunk.id).copied().unwrap_or(chunk.hit_points)),
            StructureView::Support => match support.get(&chunk.id) {
                Some(state) if state.lateral_distance.unwrap_or(0.0) > 0.0 => SUPPORT.braced_color,
                Some(state) if state.anchored => SUPPORT.anchored_color,
                _ => SUPPORT.vertical_color,
            },
            StructureView::Stress => {
                stress.get(chunk.id.as_str()).map_or(STRESS.stable_color, |state| ratio_color(state.ratio))
            }
        };

        // The telegraph: strained chunks lean amber; failing chunks pulse and shake.
        let mut offset = [0.0; 2];
        if view != StructureView::Support
            && let Some(state) = stress.get(chunk.id.as_str())
        {
            match state.stage {
                ChunkStressStage::Stable => {}
                ChunkStressStage::Strained => {
                    if view != StructureView::Stress {
                        color = lerp(color, STRESS.strained_color, STRESS.strained_tint);
                    }
                }
                ChunkStressStage::Failing => {
                    let pulse = 0.5 + (0.5 * deterministic::sin(TAU * STRESS.pulse_hertz * time_seconds));
                    color = lerp(STRESS.failing_color, [1.0; 3], pulse * 0.45);
                    offset = shake_offset(state, chunk_index, time_seconds);
                }
            }
        }

        let vertex_offset = positions.len() as u32;
        positions.extend(geometry.positions.iter().map(|position| {
            let mut position = *position;
            position.x += offset[0];
            position.z += offset[1];
            position
        }));
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
    lerp(first, second, fraction)
}

/// Load-over-capacity color: stable green to strained amber at the warning ratio, then red toward overload.
pub fn ratio_color(ratio: f64) -> [f64; 3] {
    if ratio < STRESS.warning_ratio {
        return lerp(STRESS.stable_color, STRESS.strained_color, ratio / STRESS.warning_ratio);
    }
    let overload = (ratio - STRESS.warning_ratio) / (1.0 - STRESS.warning_ratio);
    lerp(STRESS.strained_color, STRESS.failing_color, overload)
}

/// Sideways shake of a failing chunk, growing as its failure delay runs out; chunks shake out of phase.
fn shake_offset(state: &ChunkStressState, chunk_index: usize, time_seconds: f64) -> [f64; 2] {
    let remaining = state.seconds_until_failure.unwrap_or(0.0);
    let urgency = (1.0 - (remaining / STRESS.failure_delay_seconds.max(f64::EPSILON))).clamp(0.0, 1.0);
    let phase = chunk_index as f64;
    let amplitude = STRESS.shake_amplitude * (0.25 + (0.75 * urgency));
    [
        amplitude * deterministic::sin((time_seconds * SHAKE_FREQUENCIES[0]) + phase),
        amplitude * deterministic::cos((time_seconds * SHAKE_FREQUENCIES[1]) + (phase * 1.7)),
    ]
}

/// Componentwise blend from `from` to `to`, with `blend` clamped to `[0, 1]`.
fn lerp(from: [f64; 3], to: [f64; 3], blend: f64) -> [f64; 3] {
    let blend = blend.clamp(0.0, 1.0);
    [0, 1, 2].map(|axis| from[axis] + ((to[axis] - from[axis]) * blend))
}
