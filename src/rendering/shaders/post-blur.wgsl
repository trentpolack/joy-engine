// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Separable Gaussian blur for the full-image blur and the vignette's edge blur. One pass blurs along `direction`.

struct Blur {
  // Texel step along the blur axis (for example (1 / width, 0)).
  direction: vec2f,
  // Kernel radius in texels; sigma is half the radius.
  radius: f32,
  padding: f32,
};

@group(0) @binding(0) var<uniform> blur: Blur;
@group(0) @binding(1) var sourceTexture: texture_2d<f32>;
@group(0) @binding(2) var sourceSampler: sampler;

struct BlurOutput {
  @builtin(position) position: vec4f,
  @location(0) coordinate: vec2f,
};

/**
 * Full-screen triangle with top-left texture coordinates.
 */
@vertex fn vertexMain(@builtin(vertex_index) vertexIndex: u32) -> BlurOutput {
  let position = vec2f(f32((vertexIndex << 1u) & 2u), f32(vertexIndex & 2u));

  var output: BlurOutput;
  output.position = vec4f((position*2.0) - 1.0, 0.0, 1.0);
  output.coordinate = vec2f(position.x, 1.0 - position.y);
  return output;
}

/**
 * Weighted taps along one axis. Linear filtering lets each tap pair cover two texels.
 */
@fragment fn fragmentMain(input: BlurOutput) -> @location(0) vec4f {
  let radius = max(blur.radius, 0.5);
  let sigma = max(radius*0.5, 0.5);
  let samples = 12;

  var total = textureSampleLevel(sourceTexture, sourceSampler, input.coordinate, 0.0);
  var weightSum = 1.0;
  for(var index = 1; index <= samples; index++) {
    let distance = (f32(index)/f32(samples))*radius;
    let weight = exp(-(distance*distance)/(2.0*sigma*sigma));
    let offset = blur.direction*distance;
    total+= textureSampleLevel(sourceTexture, sourceSampler, input.coordinate + offset, 0.0)*weight;
    total+= textureSampleLevel(sourceTexture, sourceSampler, input.coordinate - offset, 0.0)*weight;
    weightSum+= 2.0*weight;
  }

  return total/weightSum;
}
