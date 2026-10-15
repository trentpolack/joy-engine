// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// HDR bloom pyramid: a soft-knee bright pass, 13-tap downsamples, and tent-filtered upsamples that blend wider levels
// back toward the first level by the configured scatter. Each pass draws one full-screen triangle.

struct Bloom {
  // Texel size of the texture being sampled (1 / width, 1 / height).
  sourceTexel: vec2f,
  // Upsample tent radius in source texels.
  radius: f32,
  // Contribution of the wider (lower) level during reconstruction.
  scatter: f32,
  // X threshold, Y soft knee, Z exposure, W horizontal (anamorphic) stretch.
  extract: vec4f,
};

@group(0) @binding(0) var<uniform> bloom: Bloom;
@group(0) @binding(1) var sourceTexture: texture_2d<f32>;
@group(0) @binding(2) var sourceSampler: sampler;
// The current pyramid level, blended with the upsampled wider level; unused by the bright and downsample passes.
@group(0) @binding(3) var detailTexture: texture_2d<f32>;

struct BloomOutput {
  @builtin(position) position: vec4f,
  @location(0) coordinate: vec2f,
};

/**
 * Full-screen triangle with top-left texture coordinates.
 */
@vertex fn vertexMain(@builtin(vertex_index) vertexIndex: u32) -> BloomOutput {
  let position = vec2f(f32((vertexIndex << 1u) & 2u), f32(vertexIndex & 2u));

  var output: BloomOutput;
  output.position = vec4f((position*2.0) - 1.0, 0.0, 1.0);
  output.coordinate = vec2f(position.x, 1.0 - position.y);
  return output;
}

/**
 * Thirteen-tap box-weighted downsample; overlapping kernels suppress the flicker a 2x2 box filter produces.
 */
fn downsample13(coordinate: vec2f) -> vec3f {
  let texel = bloom.sourceTexel;
  let a = textureSampleLevel(sourceTexture, sourceSampler, coordinate + (texel*vec2f(-2.0, -2.0)), 0.0).rgb;
  let b = textureSampleLevel(sourceTexture, sourceSampler, coordinate + (texel*vec2f(0.0, -2.0)), 0.0).rgb;
  let c = textureSampleLevel(sourceTexture, sourceSampler, coordinate + (texel*vec2f(2.0, -2.0)), 0.0).rgb;
  let d = textureSampleLevel(sourceTexture, sourceSampler, coordinate + (texel*vec2f(-1.0, -1.0)), 0.0).rgb;
  let e = textureSampleLevel(sourceTexture, sourceSampler, coordinate + (texel*vec2f(1.0, -1.0)), 0.0).rgb;
  let f = textureSampleLevel(sourceTexture, sourceSampler, coordinate + (texel*vec2f(-2.0, 0.0)), 0.0).rgb;
  let g = textureSampleLevel(sourceTexture, sourceSampler, coordinate, 0.0).rgb;
  let h = textureSampleLevel(sourceTexture, sourceSampler, coordinate + (texel*vec2f(2.0, 0.0)), 0.0).rgb;
  let i = textureSampleLevel(sourceTexture, sourceSampler, coordinate + (texel*vec2f(-1.0, 1.0)), 0.0).rgb;
  let j = textureSampleLevel(sourceTexture, sourceSampler, coordinate + (texel*vec2f(1.0, 1.0)), 0.0).rgb;
  let k = textureSampleLevel(sourceTexture, sourceSampler, coordinate + (texel*vec2f(-2.0, 2.0)), 0.0).rgb;
  let l = textureSampleLevel(sourceTexture, sourceSampler, coordinate + (texel*vec2f(0.0, 2.0)), 0.0).rgb;
  let m = textureSampleLevel(sourceTexture, sourceSampler, coordinate + (texel*vec2f(2.0, 2.0)), 0.0).rgb;

  var result = (d + e + i + j)*0.125;
  result+= (a + b + g + f)*0.03125;
  result+= (b + c + h + g)*0.03125;
  result+= (f + g + l + k)*0.03125;
  result+= (g + h + m + l)*0.03125;
  return result;
}

/**
 * Exposed soft-knee bright pass into the first pyramid level.
 * Brightness above `threshold` passes; the quadratic knee eases the transition so highlights do not pop.
 */
@fragment fn brightMain(input: BloomOutput) -> @location(0) vec4f {
  let color = downsample13(input.coordinate)*bloom.extract.z;
  let brightness = max(color.r, max(color.g, color.b));
  let knee = max(bloom.extract.y, 0.00001);
  var soft = clamp(brightness - bloom.extract.x + knee, 0.0, 2.0*knee);
  soft = (soft*soft)/((4.0*knee) + 0.00001);
  let contribution = max(soft, brightness - bloom.extract.x)/max(brightness, 0.00001);
  return vec4f(color*contribution, 1.0);
}

/**
 * Downsample one pyramid level into the next.
 */
@fragment fn downsampleMain(input: BloomOutput) -> @location(0) vec4f {
  return vec4f(downsample13(input.coordinate), 1.0);
}

/**
 * Tent-filter the wider level and blend it with this level's detail by the scatter amount.
 * The horizontal radius grows with the anamorphic stretch.
 */
@fragment fn upsampleMain(input: BloomOutput) -> @location(0) vec4f {
  let stretch = vec2f(1.0 + (bloom.extract.w*3.0), 1.0);
  let texel = bloom.sourceTexel*bloom.radius*stretch;
  var wide = textureSampleLevel(sourceTexture, sourceSampler, input.coordinate, 0.0).rgb*4.0;
  wide+= textureSampleLevel(sourceTexture, sourceSampler, input.coordinate + (texel*vec2f(-1.0, 0.0)), 0.0).rgb*2.0;
  wide+= textureSampleLevel(sourceTexture, sourceSampler, input.coordinate + (texel*vec2f(1.0, 0.0)), 0.0).rgb*2.0;
  wide+= textureSampleLevel(sourceTexture, sourceSampler, input.coordinate + (texel*vec2f(0.0, -1.0)), 0.0).rgb*2.0;
  wide+= textureSampleLevel(sourceTexture, sourceSampler, input.coordinate + (texel*vec2f(0.0, 1.0)), 0.0).rgb*2.0;
  wide+= textureSampleLevel(sourceTexture, sourceSampler, input.coordinate + (texel*vec2f(-1.0, -1.0)), 0.0).rgb;
  wide+= textureSampleLevel(sourceTexture, sourceSampler, input.coordinate + (texel*vec2f(1.0, -1.0)), 0.0).rgb;
  wide+= textureSampleLevel(sourceTexture, sourceSampler, input.coordinate + (texel*vec2f(-1.0, 1.0)), 0.0).rgb;
  wide+= textureSampleLevel(sourceTexture, sourceSampler, input.coordinate + (texel*vec2f(1.0, 1.0)), 0.0).rgb;
  wide/= 16.0;

  let detail = textureSampleLevel(detailTexture, sourceSampler, input.coordinate, 0.0).rgb;
  return vec4f(mix(detail, wide, bloom.scatter), 1.0);
}
