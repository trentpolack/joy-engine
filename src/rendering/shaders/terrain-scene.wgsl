// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Scene terrain material: the resident morphed surface lit like Lit meshes (ambient, skylight, the shadowed primary
// light, and point lights) into the HDR scene target. Prepended at load time by scene-common.wgsl and
// terrain-geometry.wgsl.

/**
 * Morph a grid vertex, apply the terrain's world transform, and project it with the frame camera.
 */
@vertex fn vertexMain(input: TerrainVertexInput) -> TerrainVertexOutput {
  var output = terrainSurface(input);
  output.worldPosition = (terrainModel.model*vec4f(output.worldPosition, 1.0)).xyz;
  output.position = frame.viewProjection*vec4f(output.worldPosition, 1.0);
  return output;
}

/**
 * Light the slope and altitude color with a derivative normal oriented toward the eye.
 * Derivatives are taken before any branch so they stay in uniform control flow.
 */
@fragment fn fragmentMain(input: TerrainVertexOutput) -> @location(0) vec4f {
  let surface = cross(dpdx(input.worldPosition), dpdy(input.worldPosition));
  let lines = terrainGridLines(input.levelData);
  let normal = faceEye(surface/max(length(surface), 0.000001), input.worldPosition);
  let color = terrainAlbedo(input.worldPosition.y, normal.y, input.levelData, lines);
  return vec4f(standardLighting(input.worldPosition, color, normal, terrainModel.flags.x), 1.0);
}
