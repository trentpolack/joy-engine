// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Optional Y-up masonry support approximation for chunk structures.
//!
//! Face contacts carry support upward and sideways; horizontal offsets spend the cantilever allowance. This is a
//! geometric support rule, not a stress, torque, or material-load solver. Contact topology is built once for the fixed
//! chunk collection; damage only changes which chunks are active.

use serde::{
    Deserialize,
    Serialize,
};

use super::DestructionError;
use super::chunk_structure::DestructibleChunkDefinition;
use joy_core::math::deterministic;

/// Contact tolerance used when [`ChunkSupportOptions::contact_tolerance`] is omitted, in world units.
pub const DEFAULT_CONTACT_TOLERANCE: f64 = 0.0001;

/// Horizontal axes (X and Z) in Y-up space.
const HORIZONTAL_AXES: [usize; 2] = [0, 2];

/// The vertical axis in Y-up space.
const VERTICAL_AXIS: usize = 1;

/// Ground anchoring and cantilever limits, in the same world units as chunk geometry.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkSupportOptions {
    /// World height of the ground plane; chunks whose bottom touches it (within tolerance) are anchors.
    pub ground_height: f64,
    /// Longest permitted sideways travel from an anchor to a supported chunk; nonnegative.
    pub maximum_cantilever_distance: f64,
    /// Face contact tolerance; nonnegative. `None` uses [`DEFAULT_CONTACT_TOLERANCE`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_tolerance: Option<f64>,
}

/// Copied static support diagnostics for one chunk.
///
/// A `None` distance means no permitted route to a ground anchor. Released rigid bodies are outside this graph, even
/// after they land on the ground.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkSupportState {
    /// Stable chunk id.
    pub chunk_id: String,
    /// Whether the chunk is active and rests on the ground.
    pub anchored: bool,
    /// Whether an active anchor reaches the chunk within the cantilever allowance.
    pub supported: bool,
    /// Shortest sideways travel from an anchor in world units, or `None` when unsupported.
    pub lateral_distance: Option<f64>,
}

/// One directed support contact: support flows from the source chunk to `target`, spending `distance`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SupportEdge {
    target: usize,
    distance: f64,
}

/// Immutable contact topology for a fixed collection of axis-aligned chunks, indexed in generation order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ChunkSupportGraph {
    // Chunk ids in generation order; edges and anchors share these indices.
    ids: Vec<String>,
    edges: Vec<Vec<SupportEdge>>,
    anchors: Vec<bool>,

    // Cantilever allowance in world units.
    maximum_distance: f64,
}

impl ChunkSupportGraph {
    /// Build contacts for every pair of chunks. Fails for a non-finite ground height or a negative or non-finite span
    /// or tolerance.
    pub(crate) fn new(
        chunks: &[&DestructibleChunkDefinition],
        options: ChunkSupportOptions,
    ) -> Result<Self, DestructionError> {
        let tolerance = options.contact_tolerance.unwrap_or(DEFAULT_CONTACT_TOLERANCE);
        let span = options.maximum_cantilever_distance;
        if !options.ground_height.is_finite()
            || !span.is_finite()
            || span < 0.0
            || !tolerance.is_finite()
            || tolerance < 0.0
        {
            return Err(DestructionError::InvalidSupportOptions);
        }

        // Anchors: chunks whose bottom reaches the ground while their top stays above it.
        let anchors = chunks
            .iter()
            .map(|chunk| {
                let bottom = chunk.center[VERTICAL_AXIS] - (chunk.size[VERTICAL_AXIS] / 2.0);
                let top = chunk.center[VERTICAL_AXIS] + (chunk.size[VERTICAL_AXIS] / 2.0);
                bottom <= options.ground_height + tolerance && top > options.ground_height
            })
            .collect();

        let mut edges = vec![Vec::new(); chunks.len()];
        for (source_index, source) in chunks.iter().enumerate() {
            for (target_index, target) in chunks.iter().enumerate() {
                if source.id == target.id {
                    continue;
                }
                let horizontal_distance =
                    deterministic::hypot(target.center[0] - source.center[0], target.center[2] - source.center[2]);
                let vertical_gap = (target.center[VERTICAL_AXIS] - (target.size[VERTICAL_AXIS] / 2.0))
                    - (source.center[VERTICAL_AXIS] + (source.size[VERTICAL_AXIS] / 2.0));

                // Only aligned stacks are free; staggering upward cannot bypass the sideways limit.
                let stacked = vertical_gap.abs() <= tolerance
                    && HORIZONTAL_AXES.iter().all(|axis| overlaps(source, target, *axis, tolerance));
                if stacked || shares_side_face(source, target, tolerance) {
                    edges[source_index].push(SupportEdge { target: target_index, distance: horizontal_distance });
                }
            }
        }

        Ok(Self { ids: chunks.iter().map(|chunk| chunk.id.clone()).collect(), edges, anchors, maximum_distance: span })
    }

    /// Resolve the shortest permitted sideways travel from active ground anchors.
    ///
    /// `active` is indexed in generation order. Nonnegative edge costs and a visited set bound the search to one visit
    /// per active chunk. Ties resolve to the earliest chunk in generation order, so results are deterministic.
    pub(crate) fn evaluate(&self, active: &[bool]) -> Vec<ChunkSupportState> {
        let active_count = active.iter().filter(|flag| **flag).count();
        let mut distances: Vec<f64> = (0..self.ids.len())
            .map(|index| if active[index] && self.anchors[index] { 0.0 } else { f64::INFINITY })
            .collect();
        let mut visited = vec![false; self.ids.len()];
        let mut visited_count = 0;

        while visited_count < active_count {
            let mut nearest: Option<usize> = None;
            let mut nearest_distance = f64::INFINITY;
            for index in 0..self.ids.len() {
                if active[index] && !visited[index] && distances[index] < nearest_distance {
                    nearest = Some(index);
                    nearest_distance = distances[index];
                }
            }
            let Some(nearest) = nearest.filter(|_| nearest_distance <= self.maximum_distance) else {
                break;
            };

            visited[nearest] = true;
            visited_count += 1;
            for edge in &self.edges[nearest] {
                let candidate = nearest_distance + edge.distance;
                if active[edge.target] && candidate < distances[edge.target] {
                    distances[edge.target] = candidate;
                }
            }
        }

        self.ids
            .iter()
            .enumerate()
            .map(|(index, chunk_id)| ChunkSupportState {
                chunk_id: chunk_id.clone(),
                anchored: active[index] && self.anchors[index],
                supported: visited[index],
                lateral_distance: visited[index].then_some(distances[index]),
            })
            .collect()
    }
}

/// Whether two chunks touch along an X or Z face while overlapping in the other two axes (corners never count).
fn shares_side_face(
    source: &DestructibleChunkDefinition,
    target: &DestructibleChunkDefinition,
    tolerance: f64,
) -> bool {
    HORIZONTAL_AXES.iter().any(|&axis| {
        let other_axis = 2 - axis;
        let face_gap =
            (target.center[axis] - source.center[axis]).abs() - ((source.size[axis] + target.size[axis]) / 2.0);
        face_gap.abs() <= tolerance
            && overlaps(source, target, VERTICAL_AXIS, tolerance)
            && overlaps(source, target, other_axis, tolerance)
    })
}

/// Whether two chunks overlap along one axis by more than the tolerance.
fn overlaps(
    first: &DestructibleChunkDefinition,
    second: &DestructibleChunkDefinition,
    axis: usize,
    tolerance: f64,
) -> bool {
    (first.center[axis] - second.center[axis]).abs() < ((first.size[axis] + second.size[axis]) / 2.0) - tolerance
}
