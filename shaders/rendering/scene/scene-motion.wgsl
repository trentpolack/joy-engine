// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

struct Uniforms {
  currentViewProjection: mat4x4f,
  previousViewProjection: mat4x4f,
  rasterViewProjection: mat4x4f,
  velocityJitterDelta: vec4f,
};
@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexOutput {
  @builtin(position) position: vec4f,
  @location(0) currentClip: vec4f,
  @location(1) previousClip: vec4f,
};

@vertex fn vertexMain(@location(0) position: vec3f, @location(1) previousPosition: vec3f) -> VertexOutput {
  var output: VertexOutput;
  output.position = uniforms.rasterViewProjection*vec4f(position, 1.0);
  output.currentClip = uniforms.currentViewProjection*vec4f(position, 1.0);
  output.previousClip = uniforms.previousViewProjection*vec4f(previousPosition, 1.0);
  return output;
}

struct MotionOutput {
  @location(0) unjittered: vec4f,
  @location(1) jittered: vec4f,
};

@fragment fn fragmentMain(input: VertexOutput) -> MotionOutput {
  var velocity = vec2f(0.0);
  // History behind the eye is not a valid correspondence and must never produce NaNs.
  if(input.currentClip.w > 0.000001 && input.previousClip.w > 0.000001) {
    let currentNdc = input.currentClip.xy/input.currentClip.w;
    let previousNdc = input.previousClip.xy/input.previousClip.w;
    // WebGPU framebuffer UVs run downward while clip-space Y runs upward.
    velocity = (currentNdc - previousNdc)*vec2f(0.5, -0.5);
  }
  var output: MotionOutput;
  output.unjittered = vec4f(velocity, 0.0, 1.0);
  output.jittered = vec4f(velocity + uniforms.velocityJitterDelta.xy, 0.0, 1.0);
  return output;
}
