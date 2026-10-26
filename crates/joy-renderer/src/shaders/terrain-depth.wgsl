// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Terrain depth pre-pass: the lit pass's exact invariant projection, without shading. Prepended at load time by
// scene-common.wgsl and terrain-geometry.wgsl.

/**
 * Morph a grid vertex and project it exactly as the lit terrain pass does.
 */
@vertex fn vertexMain(vertex: TerrainVertex, tile: TerrainPatch) -> @builtin(position) @invariant vec4f {
  let local = vec4f(terrainPosition(vertex.gridPosition, tile), 1.0);
  let world = terrainModel.model*local;
  return frame.viewProjection*vec4f(world.xyz, 1.0);
}
