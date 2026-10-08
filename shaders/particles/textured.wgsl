// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Camera-facing particle material.
 * Texture RGB is sampled as sRGB by the GPU and multiplied by linear HDR vertex tint; alpha remains straight.
 */

struct Uniforms {
  viewProjection: mat4x4<f32>,
  tint: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

@group(0) @binding(1) var particleTexture: texture_2d<f32>;
@group(0) @binding(2) var particleTextureSampler: sampler;

struct VertexOutput {
  @builtin(position) position: vec4<f32>,
  @location(0) color: vec4<f32>,
  @location(1) uv: vec2<f32>,
};

@vertex
fn vertexMain(
  @location(0) position: vec3<f32>,
  @location(1) color: vec4<f32>,
  @location(2) uv: vec2<f32>,
) -> VertexOutput {
  var output: VertexOutput;
  output.position = vec4<f32>(position, 1.0)*uniforms.viewProjection;
  output.color = color*uniforms.tint;
  output.uv = uv;

  return output;
}

@fragment
fn fragmentMain(input: VertexOutput) -> @location(0) vec4<f32> {
  let sourceColor = textureSample(particleTexture, particleTextureSampler, input.uv)*input.color;
  return sourceColor;
}
