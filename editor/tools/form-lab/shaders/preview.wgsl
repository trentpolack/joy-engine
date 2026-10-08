// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * FORM geometry supplies world-space positions and linear vertex colors.
 * The preview has no model transform because authored output already uses world space.
 */

// The viewport supplies the first 80 bytes; Joy Engine appends the post-processing uniform tail.
struct Uniforms {
  viewProjection: mat4x4<f32>,
  tint: vec4<f32>,
  // JOY_POST_UNIFORMS_MACRO
};
@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexInput {
  @location(0) position: vec3<f32>,
  @location(1) color: vec4<f32>,
};

struct VertexOutput {
  @builtin(position) position: vec4<f32>,
  @location(0) color: vec4<f32>,
};

@vertex fn vertexMain(input: VertexInput) -> VertexOutput {
  var output: VertexOutput;
  output.position = uniforms.viewProjection * vec4<f32>(input.position, 1.0);

  // Apply the preview tint per vertex so the fragment stage stays a simple, interpolation-only pass on both rendering backends.
  output.color = input.color*uniforms.tint;
  return output;
}

// JOY_POST_DECLARATIONS_MACRO

@fragment fn fragmentMain(input: VertexOutput) -> @location(0) vec4<f32> {
  // Alpha is preserved for the viewport blend state to resolve.
  let sourceColor = input.color;
  // JOY_POST_APPLY_MACRO
}
