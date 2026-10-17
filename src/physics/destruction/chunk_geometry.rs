// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Renderer-free box geometry for chunk structures.

use serde_json::Value;

use super::chunk_structure::DestructibleChunkDefinition;
use crate::core::{
    JsonRecord,
    MeshAssetData,
};

/// Material whose chunks draw with the dark frame tint.
pub const FRAME_MATERIAL: &str = "frame";

/// Material whose chunks draw with the interior tint.
pub const INTERIOR_MATERIAL: &str = "interior";

/// Linear RGB tint for [`FRAME_MATERIAL`] chunks.
const FRAME_TINT: [f64; 3] = [0.18, 0.22, 0.26];

/// Linear RGB tint for [`INTERIOR_MATERIAL`] chunks.
const INTERIOR_TINT: [f64; 3] = [0.42, 0.2, 0.08];

/// Linear RGB tint for every other material.
const DEFAULT_TINT: [f64; 3] = [0.94, 0.32, 0.035];

/// Metadata key holding the number of chunks in the mesh.
const CHUNK_COUNT_KEY: &str = "chunkCount";

/// Metadata key marking that every box is closed, so removed neighbors reveal interior faces.
const CLOSED_INTERIOR_SURFACES_KEY: &str = "closedInteriorSurfaces";

/// Box corners per face as unit offsets, wound like the TypeScript faces (-Z, +Z, -X, +X, -Y, +Y).
const FACE_CORNERS: [[[f64; 3]; 4]; 6] = [
    [[-1.0, -1.0, -1.0], [1.0, -1.0, -1.0], [1.0, 1.0, -1.0], [-1.0, 1.0, -1.0]],
    [[-1.0, -1.0, 1.0], [-1.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, -1.0, 1.0]],
    [[-1.0, -1.0, -1.0], [-1.0, 1.0, -1.0], [-1.0, 1.0, 1.0], [-1.0, -1.0, 1.0]],
    [[1.0, -1.0, -1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, -1.0]],
    [[-1.0, -1.0, -1.0], [-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, -1.0, -1.0]],
    [[-1.0, 1.0, -1.0], [1.0, 1.0, -1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]],
];

/// Build closed box geometry (four vertices and two triangles per face) for the given chunks, in iteration order.
///
/// Every chunk is a closed box, so removing a neighbor reveals an authored interior face. Positions are world units,
/// colors are per-vertex linear RGB material tints, and normals are left to the consumer (flat faces). Metadata
/// records `chunkCount` and `closedInteriorSurfaces: true`. The result is plain data owned by the caller.
pub fn create_chunk_mesh_data<'a>(chunks: impl IntoIterator<Item = &'a DestructibleChunkDefinition>) -> MeshAssetData {
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    let mut colors = Vec::new();
    let mut chunk_count: u64 = 0;

    for chunk in chunks {
        chunk_count += 1;
        let tint = material_tint(&chunk.material);
        for corners in &FACE_CORNERS {
            // Meshes this large exceed GPU index ranges long before u32.
            let start = (positions.len() / 3) as u32;
            for corner in corners {
                for (axis, offset) in corner.iter().enumerate() {
                    positions.push(chunk.center[axis] + ((offset * chunk.size[axis]) / 2.0));
                }
                colors.extend_from_slice(&tint);
            }
            indices.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
        }
    }

    let mut metadata = JsonRecord::new();
    metadata.insert(CHUNK_COUNT_KEY.to_owned(), Value::from(chunk_count));
    metadata.insert(CLOSED_INTERIOR_SURFACES_KEY.to_owned(), Value::Bool(true));
    MeshAssetData { positions, indices, colors: Some(colors), metadata: Some(metadata), ..Default::default() }
}

/// Built-in chunk tint for a material name.
fn material_tint(material: &str) -> [f64; 3] {
    match material {
        FRAME_MATERIAL => FRAME_TINT,
        INTERIOR_MATERIAL => INTERIOR_TINT,
        _ => DEFAULT_TINT,
    }
}
