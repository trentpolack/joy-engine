// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Terrain depth pre-pass: the lit pass's exact invariant projection, without shading. Prepended at load time by
// scene-common.wgsl and terrain-geometry.wgsl.

/**
 * Morph a grid vertex and project it exactly as the lit terrain pass does.
 */
@vertex fn vertexMain(input: TerrainVertexInput) -> @builtin(position) @invariant vec4f {
  let surface = terrainSurface(input);
  let world = (terrainModel.model*vec4f(surface.worldPosition, 1.0)).xyz;
  return frame.viewProjection*vec4f(world, 1.0);
}
