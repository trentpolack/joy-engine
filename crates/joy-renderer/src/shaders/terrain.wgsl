// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Standalone terrain pass: derivative-normal diffuse terrain coloring with display gamma, drawn straight into the
// caller's presentation target. Prepended at load time by terrain-geometry.wgsl.

/**
 * Morph a grid vertex and project it with the standalone view-projection.
 */
@vertex fn vertexMain(input: TerrainVertexInput) -> TerrainVertexOutput {
  return terrainSurface(input);
}

/**
 * Shade the morphed surface and optional LOD diagnostics.
 * Normals come from the rendered triangle derivatives, so lighting follows the continuously morphed geometry on screen.
 */
@fragment fn fragmentMain(input: TerrainVertexOutput) -> @location(0) vec4f {
  // Orient upward because either winding can be visible with culling disabled.
  var n = normalize(cross(dpdx(input.worldPosition), dpdy(input.worldPosition)));
  if(n.y < 0.0) {
    n = -n;
  }
  let lines = terrainGridLines(input.levelData);

  // Simple slope/altitude coloring and diffuse light; this is not a PBR material.
  let light = 0.3 + (0.7*max(0.0, dot(n, normalize(vec3f(-0.4, 0.8, 0.2)))));
  let color = terrainAlbedo(input.worldPosition.y, n.y, input.levelData, lines)*light;

  // Apply display gamma for this standalone pass.
  return vec4f(pow(color, vec3f(1.0/2.2)), 1.0);
}
