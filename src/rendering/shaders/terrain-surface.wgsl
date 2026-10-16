// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Scene effect attachment for terrain: view-space normals and roughness, drawn against the scene's depth with an equal
// test. The vertex transform must match terrain-scene.wgsl exactly. Prepended at load time by scene-common.wgsl and
// terrain-geometry.wgsl.

/**
 * Project the morphed surface with the scene pass's exact expression so depth matches.
 */
@vertex fn vertexMain(input: TerrainVertexInput) -> TerrainVertexOutput {
  var output = terrainSurface(input);
  output.worldPosition = (terrainModel.model*vec4f(output.worldPosition, 1.0)).xyz;
  output.position = frame.viewProjection*vec4f(output.worldPosition, 1.0);
  return output;
}

/**
 * Encode the derivative normal in view space, oriented toward the eye, with the terrain's roughness in alpha.
 */
@fragment fn fragmentMain(input: TerrainVertexOutput) -> @location(0) vec4f {
  let surface = cross(dpdx(input.worldPosition), dpdy(input.worldPosition));
  let worldNormal = surface/max(length(surface), 0.000001);
  let transformed = (frame.view*vec4f(worldNormal, 0.0)).xyz;
  var viewNormal = transformed/max(length(transformed), 0.000001);
  let viewPosition = (frame.view*vec4f(input.worldPosition, 1.0)).xyz;
  if(dot(viewNormal, -viewPosition) < 0.0) {
    viewNormal = -viewNormal;
  }
  return vec4f((viewNormal*0.5) + 0.5, clamp(terrainModel.flags.y, 0.0, 1.0));
}
