// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Depth-only directional shadow caster for terrain: the same indexed, morphed surface the camera sees, projected into
// the light's volume. Prepended at load time by terrain-geometry.wgsl.

struct TerrainCaster {
  lightViewProjection: mat4x4f,
};

@group(0) @binding(0) var<uniform> caster: TerrainCaster;

/**
 * Morph a grid vertex, apply the world transform, and project it into the light's orthographic volume.
 */
@vertex fn vertexMain(input: TerrainVertexInput) -> @builtin(position) vec4f {
  let surface = terrainSurface(input);
  return caster.lightViewProjection*(terrainModel.model*vec4f(surface.worldPosition, 1.0));
}
