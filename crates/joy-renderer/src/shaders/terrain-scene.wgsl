// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Lit scene terrain: the engine's standard flat lighting with directional and point-light shadows, plus the scene-effect
// attachments when effects need them. Prepended at load time by scene-common.wgsl and terrain-geometry.wgsl.

/**
 * Morph a grid vertex, apply the terrain's world transform, and project it with the frame camera.
 */
@vertex fn vertexMain(input: TerrainVertexInput) -> TerrainVertexOutput {
  var output = terrainSurface(input);
  let local = vec4f(output.worldPosition, 1.0);
  output.worldPosition = (terrainModel.model*local).xyz;
  output.position = frame.viewProjection*vec4f(output.worldPosition, 1.0);
  let clips = motionClips(vec4f(output.worldPosition, 1.0), terrainModel.previousModel*local);
  output.currentClip = clips[0];
  output.previousClip = clips[1];
  return output;
}

/**
 * Light the slope and altitude color with a derivative normal oriented toward the eye. `surface` and `lines` are
 * derivative values the entry point takes in uniform control flow.
 */
fn terrainColor(input: TerrainVertexOutput, surface: vec3f, lines: vec2f) -> vec4f {
  let normal = faceEye(surface/max(length(surface), 0.000001), input.worldPosition);
  let color = terrainAlbedo(input.worldPosition.y, normal.y, input.levelData, lines);
  return vec4f(standardLighting(input.worldPosition, color, normal, terrainModel.flags.x), 1.0);
}

/**
 * Color only.
 */
@fragment fn fragmentMain(input: TerrainVertexOutput) -> @location(0) vec4f {
  let surface = cross(dpdx(input.worldPosition), dpdy(input.worldPosition));
  let lines = terrainGridLines(input.levelData);
  return terrainColor(input, surface, lines);
}

/**
 * Color plus the scene-effect attachments, with the terrain's roughness.
 */
@fragment fn fragmentTargets(input: TerrainVertexOutput) -> SceneTargets {
  let surface = cross(dpdx(input.worldPosition), dpdy(input.worldPosition));
  let lines = terrainGridLines(input.levelData);
  let clips = mat2x4f(input.currentClip, input.previousClip);
  return sceneTargets(terrainColor(input, surface, lines), input.worldPosition, surface, terrainModel.flags.y, clips);
}
