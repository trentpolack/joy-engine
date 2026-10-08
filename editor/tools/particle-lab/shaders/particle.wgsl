// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

struct Uniforms {
  viewProjection: mat4x4<f32>,
  tint: vec4<f32>,
  // JOY_POST_UNIFORMS_MACRO
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct VertexOutput {
  @builtin(position) position: vec4<f32>,
  @location(0) color: vec4<f32>,
};

// JOY_POST_DECLARATIONS_MACRO

@vertex
fn vertexMain(
  @location(0) position: vec3<f32>,
  @location(1) color: vec4<f32>,
) -> VertexOutput {
  var output: VertexOutput;
  output.position = uniforms.viewProjection*vec4<f32>(position, 1.0);
  output.color = color*uniforms.tint;
  return output;
}

@fragment
fn fragmentMain(input: VertexOutput) -> @location(0) vec4<f32> {
  let sourceColor = input.color;
  // JOY_POST_APPLY_MACRO
}
