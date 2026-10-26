// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Depth-only shadow caster for terrain: the same indexed, morphed surface the camera sees, projected into one shadow
// view (a directional cascade or a point-light face). Prepended at load time by terrain-geometry.wgsl.

struct TerrainCaster {
  lightViewProjection: mat4x4f,
};

@group(0) @binding(0) var<uniform> caster: TerrainCaster;

/**
 * Morph a grid vertex, apply the world transform, and project it into the shadow view.
 */
@vertex fn vertexMain(vertex: TerrainVertex, tile: TerrainPatch) -> @builtin(position) vec4f {
  let local = vec4f(terrainPosition(vertex.gridPosition, tile), 1.0);
  return caster.lightViewProjection*(terrainModel.model*local);
}
