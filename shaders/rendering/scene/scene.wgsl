// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

struct Uniforms {
  viewProjection: mat4x4f,
  tint: vec4f,
  lighting: vec4f,
  lights: array<vec4f, 64>,
  atmosphereLighting: array<vec4f, 3>,
  // JOY_POST_UNIFORMS_MACRO
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
struct VertexOutput {
  @builtin(position) position: vec4f,
  @location(0) color: vec4f,
};

@vertex fn vertexMain(@location(0) position: vec3f, @location(1) color: vec4f) -> VertexOutput {
  var output: VertexOutput;
  output.position = uniforms.viewProjection*vec4f(position, 1.0);
  output.color = color*uniforms.tint;

  return output;
}

// JOY_POST_DECLARATIONS_MACRO

@fragment fn fragmentMain(input: VertexOutput) -> @location(0) vec4f {
  let sourceColor = input.color;
  // JOY_POST_APPLY_MACRO
}
