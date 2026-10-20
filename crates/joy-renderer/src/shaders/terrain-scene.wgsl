// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Lit scene terrain: the engine's standard flat lighting with directional and point-light shadows, plus the scene-effect
// attachments when effects need them. In deferred shading terrain writes the lit model's G-buffer encoding instead
// (its color), and the lit model's lighting pass adds the direct light. Prepended at load time by scene-common.wgsl,
// shadow-receiver.wgsl, gbuffer-encoding.wgsl, and terrain-geometry.wgsl.

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
 * The derivative normal oriented toward the eye, and the slope and altitude color under it. `surface` and `lines` are
 * derivative values the entry point takes in uniform control flow.
 */
fn terrainShading(input: TerrainVertexOutput, surface: vec3f, lines: vec2f) -> TerrainShading {
  var shading: TerrainShading;
  shading.normal = faceEye(surface/max(length(surface), 0.000001), input.worldPosition);
  shading.albedo = terrainAlbedo(input.worldPosition.y, shading.normal.y, input.levelData, lines);
  return shading;
}

/**
 * The fully lit terrain color.
 */
fn terrainColor(input: TerrainVertexOutput, surface: vec3f, lines: vec2f) -> vec4f {
  let shading = terrainShading(input, surface, lines);
  return vec4f(standardLighting(input.worldPosition, shading.albedo, shading.normal, terrainModel.flags.x), 1.0);
}

struct TerrainShading {
  normal: vec3f,
  albedo: vec3f,
};

struct TerrainGBuffer {
  @location(0) color: vec4f,
  @location(1) gbuffer: vec4u,
};

struct TerrainGBufferTargets {
  @location(0) color: vec4f,
  @location(1) motion: vec4f,
  @location(2) gbuffer: vec4u,
};

/**
 * Ambient and skylight on the albedo, and the lit model's G-buffer texel.
 */
fn terrainGBuffer(input: TerrainVertexOutput, surface: vec3f, lines: vec2f) -> TerrainGBuffer {
  let shading = terrainShading(input, surface, lines);
  var targets: TerrainGBuffer;
  targets.color = vec4f(shading.albedo*standardAmbient(shading.normal), 1.0);
  let header = gbufferHeader(shading.normal, terrainModel.flags.x > 0.5);
  targets.gbuffer = vec4u(header, packRgb9e5(shading.albedo), 0u, 0u);
  return targets;
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
 * Color plus the scene-effect motion attachment.
 */
@fragment fn fragmentTargets(input: TerrainVertexOutput) -> SceneTargets {
  let surface = cross(dpdx(input.worldPosition), dpdy(input.worldPosition));
  let lines = terrainGridLines(input.levelData);
  let clips = mat2x4f(input.currentClip, input.previousClip);
  return sceneTargets(terrainColor(input, surface, lines), input.worldPosition, clips);
}

/**
 * Scene color and G-buffer.
 */
@fragment fn fragmentGBuffer(input: TerrainVertexOutput) -> TerrainGBuffer {
  let surface = cross(dpdx(input.worldPosition), dpdy(input.worldPosition));
  let lines = terrainGridLines(input.levelData);
  return terrainGBuffer(input, surface, lines);
}

/**
 * Scene color, the scene-effect motion attachment, and G-buffer.
 */
@fragment fn fragmentGBufferTargets(input: TerrainVertexOutput) -> TerrainGBufferTargets {
  let surface = cross(dpdx(input.worldPosition), dpdy(input.worldPosition));
  let lines = terrainGridLines(input.levelData);
  let written = terrainGBuffer(input, surface, lines);
  let clips = mat2x4f(input.currentClip, input.previousClip);
  let scene = sceneTargets(written.color, input.worldPosition, clips);
  var targets: TerrainGBufferTargets;
  targets.color = scene.color;
  targets.motion = scene.motion;
  targets.gbuffer = written.gbuffer;
  return targets;
}
