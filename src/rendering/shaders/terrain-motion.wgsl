// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Scene effect attachment for terrain: screen-space motion from camera and world-transform changes. LOD morphs use the
// current surface for both positions, so selection changes never create false world motion. Prepended at load time by
// terrain-geometry.wgsl.

struct TerrainMotionUniforms {
  currentViewProjection: mat4x4f,
  previousViewProjection: mat4x4f,
  rasterViewProjection: mat4x4f,
  currentModel: mat4x4f,
  previousModel: mat4x4f,
  // XY jitter change since the previous frame, in UV units.
  jitterDelta: vec4f,
};

@group(0) @binding(0) var<uniform> motion: TerrainMotionUniforms;

struct TerrainMotionOutput {
  @builtin(position) position: vec4f,
  @location(0) currentClip: vec4f,
  @location(1) previousClip: vec4f,
};

/**
 * Project the morphed surface with the current and previous camera and transform.
 */
@vertex fn vertexMain(input: TerrainVertexInput) -> TerrainMotionOutput {
  let surface = terrainSurface(input);
  let local = vec4f(surface.worldPosition, 1.0);
  let currentWorld = motion.currentModel*local;
  let previousWorld = motion.previousModel*local;
  var output: TerrainMotionOutput;
  output.position = motion.rasterViewProjection*currentWorld;
  output.currentClip = motion.currentViewProjection*currentWorld;
  output.previousClip = motion.previousViewProjection*previousWorld;
  return output;
}

struct TerrainMotionTargets {
  @location(0) unjittered: vec4f,
  @location(1) jittered: vec4f,
};

/**
 * Encode per-pixel screen-space motion; history behind the eye produces none.
 */
@fragment fn fragmentMain(input: TerrainMotionOutput) -> TerrainMotionTargets {
  var velocity = vec2f(0.0);
  if((input.currentClip.w > 0.000001) && (input.previousClip.w > 0.000001)) {
    velocity = ((input.currentClip.xy/input.currentClip.w) - (input.previousClip.xy/input.previousClip.w))*vec2f(0.5, -0.5);
  }
  var output: TerrainMotionTargets;
  output.unjittered = vec4f(velocity, 0.0, 1.0);
  output.jittered = vec4f(velocity + motion.jitterDelta.xy, 0.0, 1.0);
  return output;
}
