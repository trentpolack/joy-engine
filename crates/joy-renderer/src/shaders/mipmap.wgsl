// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Mip chain generation: each level is the previous level resampled at half resolution with linear filtering, a 2x2 box
// average. sRGB textures are read and written through sRGB views, so the average happens in linear light.

@group(0) @binding(0) var sourceLevel: texture_2d<f32>;
@group(0) @binding(1) var linearSampler: sampler;

struct MipmapOutput {
  @builtin(position) position: vec4f,
  @location(0) coordinate: vec2f,
};

/**
 * Full-screen triangle with top-left texture coordinates.
 */
@vertex fn vertexMain(@builtin(vertex_index) vertexIndex: u32) -> MipmapOutput {
  let position = vec2f(f32((vertexIndex << 1u) & 2u), f32(vertexIndex & 2u));

  var output: MipmapOutput;
  output.position = vec4f((position*2.0) - 1.0, 0.0, 1.0);
  output.coordinate = vec2f(position.x, 1.0 - position.y);
  return output;
}

/**
 * Average the four source texels under this destination texel.
 */
@fragment fn fragmentMain(input: MipmapOutput) -> @location(0) vec4f {
  return textureSampleLevel(sourceLevel, linearSampler, input.coordinate, 0.0);
}
