// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Scene effect attachment: screen-space motion vectors for opaque geometry, from each instance's current and previous
// model matrices and the current and previous camera.

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

/**
 * Project geometry for the motion pass, keeping current and previous clip positions for the fragment stage.
 */
@vertex fn vertexMain(
  @location(0) position: vec3f,
  @location(5) model0: vec4f,
  @location(6) model1: vec4f,
  @location(7) model2: vec4f,
  @location(8) model3: vec4f,
  @location(11) previous0: vec4f,
  @location(12) previous1: vec4f,
  @location(13) previous2: vec4f,
  @location(14) previous3: vec4f
  ) -> VertexOutput {
  let world = mat4x4f(model0, model1, model2, model3)*vec4f(position, 1.0);
  let previousWorld = mat4x4f(previous0, previous1, previous2, previous3)*vec4f(position, 1.0);
  var output: VertexOutput;
  output.position = uniforms.rasterViewProjection*world;
  output.currentClip = uniforms.currentViewProjection*world;
  output.previousClip = uniforms.previousViewProjection*previousWorld;
  return output;
}

struct MotionOutput {
  @location(0) unjittered: vec4f,
  @location(1) jittered: vec4f,
};

/**
 * Encode per-pixel screen-space motion; the perspective divide happens per fragment.
 */
@fragment fn fragmentMain(input: VertexOutput) -> MotionOutput {
  var velocity = vec2f(0.0);

  // History behind the eye is not a valid correspondence and must never produce NaNs.
  if((input.currentClip.w > 0.000001) && (input.previousClip.w > 0.000001)) {
    let currentNdc = input.currentClip.xy/input.currentClip.w;
    let previousNdc = input.previousClip.xy/input.previousClip.w;

    // Framebuffer UVs run downward while clip-space Y runs upward.
    velocity = (currentNdc - previousNdc)*vec2f(0.5, -0.5);
  }

  var output: MotionOutput;
  output.unjittered = vec4f(velocity, 0.0, 1.0);
  output.jittered = vec4f(velocity + uniforms.velocityJitterDelta.xy, 0.0, 1.0);
  return output;
}
