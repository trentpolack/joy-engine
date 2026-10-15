// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

struct TerrainMotionUniforms {
  currentViewProjection: mat4x4f,
  previousViewProjection: mat4x4f,
  rasterViewProjection: mat4x4f,
  currentModel: mat4x4f,
  previousModel: mat4x4f,
  jitterDelta: vec4f,
};
@group(0) @binding(0) var<uniform> attachment: TerrainMotionUniforms;

struct TerrainMotionOutput {
  @builtin(position) position: vec4f,
  @location(0) currentClip: vec4f,
  @location(1) previousClip: vec4f,
};

@vertex fn vertexMain(input: VertexInput) -> TerrainMotionOutput {
  let surface = terrainSurface(input);
  let local = vec4f(surface.worldPosition, 1.0);
  let currentWorld = attachment.currentModel*local;
  let previousWorld = attachment.previousModel*local;
  var output: TerrainMotionOutput;
  output.position = attachment.rasterViewProjection*currentWorld;
  output.currentClip = attachment.currentViewProjection*currentWorld;
  output.previousClip = attachment.previousViewProjection*previousWorld;
  return output;
}

struct MotionOutput {
  @location(0) unjittered: vec4f,
  @location(1) jittered: vec4f,
};

@fragment fn fragmentMain(input: TerrainMotionOutput) -> MotionOutput {
  var velocity = vec2f(0.0);
  if((input.currentClip.w > 0.000001) && (input.previousClip.w > 0.000001)) {
    velocity = ((input.currentClip.xy/input.currentClip.w) - (input.previousClip.xy/input.previousClip.w))*vec2f(0.5, -0.5);
  }
  var output: MotionOutput;
  output.unjittered = vec4f(velocity, 0.0, 1.0);
  output.jittered = vec4f(velocity + attachment.jitterDelta.xy, 0.0, 1.0);
  return output;
}
