// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// HDR bloom as a mip pyramid (after Jimenez 2014, "Next Generation Post Processing in Call of Duty: Advanced Warfare").
// The prefilter exposes the scene, soft-thresholds it, and downsamples it into the first level; each further level
// downsamples the one above it; then each level from the coarsest inward is upsampled with a tent filter and blended
// into the next finer level (the pipeline's constant blend factor sets how much of the wider glow it takes). The finest
// level ends up holding the glow the resolve adds to the scene.
//
// Filters by quality: low reads four texels per downsample and upsample; higher qualities use the thirteen-tap
// downsample and nine-tap tent, and the prefilter can average its five boxes by inverse luminance (Karis) so a few very
// bright pixels cannot make the glow flicker as they move.

struct Bloom {
  // XY source texel size in UV, Z threshold (exposed linear luminance where glow begins), W soft knee width.
  source: vec4f,
  // X manual exposure, Y filter quality (0 four-tap, 1 thirteen-tap, 2 thirteen-tap with firefly suppression), ZW
  // unused.
  pyramid: vec4f,
};

@group(0) @binding(0) var<uniform> bloom: Bloom;
@group(0) @binding(1) var sourceTexture: texture_2d<f32>;
@group(0) @binding(2) var linearSampler: sampler;
@group(0) @binding(3) var exposureTexture: texture_2d<f32>;

struct BloomVarying {
  @builtin(position) position: vec4f,
  @location(0) coordinate: vec2f,
};

/**
 * Full-screen triangle with top-left texture coordinates.
 */
@vertex fn vertexMain(@builtin(vertex_index) vertexIndex: u32) -> BloomVarying {
  let position = vec2f(f32((vertexIndex << 1u) & 2u), f32(vertexIndex & 2u));
  var output: BloomVarying;
  output.position = vec4f((position*2.0) - 1.0, 0.0, 1.0);
  output.coordinate = vec2f(position.x, 1.0 - position.y);
  return output;
}

fn sampleSource(coordinate: vec2f, offset: vec2f) -> vec3f {
  return textureSampleLevel(sourceTexture, linearSampler, coordinate + (offset*bloom.source.xy), 0.0).rgb;
}

fn luminance(color: vec3f) -> f32 {
  return dot(color, vec3f(0.2126, 0.7152, 0.0722));
}

/**
 * Weight of a box average under Karis firefly suppression.
 */
fn karisWeight(color: vec3f) -> f32 {
  return 1.0/(1.0 + luminance(color));
}

/**
 * Thirteen bilinear fetches covering a 6×6 texel footprint: a center box weighted one half and four overlapping
 * corner boxes weighted one eighth each. With `suppressFireflies`, the boxes are averaged by inverse luminance instead.
 */
fn downsampleThirteen(coordinate: vec2f, suppressFireflies: bool) -> vec3f {
  let a = sampleSource(coordinate, vec2f(-2.0, -2.0));
  let b = sampleSource(coordinate, vec2f(0.0, -2.0));
  let c = sampleSource(coordinate, vec2f(2.0, -2.0));
  let d = sampleSource(coordinate, vec2f(-2.0, 0.0));
  let e = sampleSource(coordinate, vec2f(0.0, 0.0));
  let f = sampleSource(coordinate, vec2f(2.0, 0.0));
  let g = sampleSource(coordinate, vec2f(-2.0, 2.0));
  let h = sampleSource(coordinate, vec2f(0.0, 2.0));
  let i = sampleSource(coordinate, vec2f(2.0, 2.0));
  let j = sampleSource(coordinate, vec2f(-1.0, -1.0));
  let k = sampleSource(coordinate, vec2f(1.0, -1.0));
  let l = sampleSource(coordinate, vec2f(-1.0, 1.0));
  let m = sampleSource(coordinate, vec2f(1.0, 1.0));

  let center = (j + k + l + m)*0.25;
  let topLeft = (a + b + d + e)*0.25;
  let topRight = (b + c + e + f)*0.25;
  let bottomLeft = (d + e + g + h)*0.25;
  let bottomRight = (e + f + h + i)*0.25;
  if(!suppressFireflies) {
    return (center*0.5) + ((topLeft + topRight + bottomLeft + bottomRight)*0.125);
  }

  let centerWeight = karisWeight(center)*0.5;
  let topLeftWeight = karisWeight(topLeft)*0.125;
  let topRightWeight = karisWeight(topRight)*0.125;
  let bottomLeftWeight = karisWeight(bottomLeft)*0.125;
  let bottomRightWeight = karisWeight(bottomRight)*0.125;
  let total = centerWeight + topLeftWeight + topRightWeight + bottomLeftWeight + bottomRightWeight;
  let sum = (center*centerWeight) + (topLeft*topLeftWeight) + (topRight*topRightWeight)
    + (bottomLeft*bottomLeftWeight) + (bottomRight*bottomRightWeight);
  return sum/total;
}

/**
 * Four texels in one bilinear fetch at their shared corner.
 */
fn downsampleFour(coordinate: vec2f) -> vec3f {
  return sampleSource(coordinate, vec2f(0.0));
}

fn downsample(coordinate: vec2f, suppressFireflies: bool) -> vec3f {
  if(bloom.pyramid.y < 0.5) {
    return downsampleFour(coordinate);
  }
  return downsampleThirteen(coordinate, suppressFireflies);
}

/**
 * Exposed scene light above the threshold, eased in over the soft knee (a quadratic ramp, then linear).
 */
fn threshold(color: vec3f) -> vec3f {
  let brightness = max(color.r, max(color.g, color.b));
  let knee = bloom.source.w;
  var soft = clamp(brightness - bloom.source.z + knee, 0.0, 2.0*knee);
  soft = (soft*soft)/((4.0*knee) + 0.00001);
  let contribution = max(soft, brightness - bloom.source.z)/max(brightness, 0.00001);
  return color*contribution;
}

/**
 * First level: exposed, thresholded scene light.
 */
@fragment fn prefilterMain(input: BloomVarying) -> @location(0) vec4f {
  let exposure = bloom.pyramid.x*textureLoad(exposureTexture, vec2i(0), 0).r;
  let color = downsample(input.coordinate, bloom.pyramid.y > 1.5)*exposure;
  return vec4f(threshold(color), 1.0);
}

/**
 * Further levels: the previous level, downsampled.
 */
@fragment fn downsampleMain(input: BloomVarying) -> @location(0) vec4f {
  return vec4f(downsample(input.coordinate, false), 1.0);
}

/**
 * The coarser level, tent-filtered up; the pipeline blends it into the finer level by the blend constant.
 */
@fragment fn upsampleMain(input: BloomVarying) -> @location(0) vec4f {
  let coordinate = input.coordinate;
  if(bloom.pyramid.y < 0.5) {
    let diagonal = (sampleSource(coordinate, vec2f(-0.5, -0.5)) + sampleSource(coordinate, vec2f(0.5, -0.5))
      + sampleSource(coordinate, vec2f(-0.5, 0.5)) + sampleSource(coordinate, vec2f(0.5, 0.5)))*0.25;
    return vec4f(diagonal, 1.0);
  }

  // 3×3 tent: corners one, edges two, center four, over sixteen.
  var color = sampleSource(coordinate, vec2f(0.0))*4.0;
  color+= (sampleSource(coordinate, vec2f(-1.0, 0.0)) + sampleSource(coordinate, vec2f(1.0, 0.0))
    + sampleSource(coordinate, vec2f(0.0, -1.0)) + sampleSource(coordinate, vec2f(0.0, 1.0)))*2.0;
  color+= sampleSource(coordinate, vec2f(-1.0, -1.0)) + sampleSource(coordinate, vec2f(1.0, -1.0))
    + sampleSource(coordinate, vec2f(-1.0, 1.0)) + sampleSource(coordinate, vec2f(1.0, 1.0));
  return vec4f(color/16.0, 1.0);
}
