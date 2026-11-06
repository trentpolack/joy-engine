// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// A triangle shader in the shape BLAST GARDEN and SPACE SCUFFLES author: an 80-byte block (camera matrix plus frame
// values), vertex colors carried as display (sRGB) values and decoded to linear light, and the engine's post marker.

struct Uniforms {
  viewProjection: mat4x4f,
  frame: vec4f,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexOutput {
  @builtin(position) position: vec4f,
  @location(0) color: vec4f,
};

/**
 * Decode a display (sRGB) color channel triple to linear light.
 */
fn decodeSrgb(encoded: vec3f) -> vec3f {
  let magnitude = abs(encoded);
  let curve = pow((magnitude + vec3f(0.055))/1.055, vec3f(2.4));
  let segment = magnitude/12.92;
  return sign(encoded)*select(curve, segment, magnitude <= vec3f(0.04045));
}

@vertex
fn vertexMain(@location(0) position: vec3f, @location(1) color: vec4f) -> VertexOutput {
  var output: VertexOutput;
  output.position = uniforms.viewProjection*vec4f(position, 1.0);
  output.color = vec4f(decodeSrgb(color.rgb), color.a);
  return output;
}

@fragment
fn fragmentMain(input: VertexOutput) -> @location(0) vec4f {
  let sourceColor = input.color;
  // JOY_POST_APPLY_MACRO
}
