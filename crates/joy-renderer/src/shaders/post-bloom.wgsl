// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// HDR multiscale bloom with lens starburst streaks: the fragment path of luma.gl 9.4.2's
// `createBloomShaderPassPipeline` (@luma.gl/effects src/passes/postprocessing/image-blur-filters/
// bloom-shader-pass-pipeline.ts and bloom-lens-effects.ts), which the TypeScript engine runs. Ported from luma.gl,
// Copyright (c) vis.gl contributors, MIT License; see src/rendering/scene_effects/LICENSE-luma.gl.
//
// Each pyramid level is extracted (first level) or downsampled with a footprint-adaptive tent, then blurred by two
// separable Gaussians. Reconstruction runs from the widest level inward: a four-fetch bicubic B-spline of the wider glow
// blended with this level's blur by scatter. The starburst streaks the reconstructed glow, and the composite adds glow
// and streaks to the scene. Each pass draws one full-screen triangle. Fixed luma.gl options the engine never changes
// (firefly reduction, exposure compensation, tint, rotation, ghosts, halo, dirt) are folded in as constants.

struct Bloom {
  // luma.gl's texture transform: the source-over-output aspect correction, applied around the image center.
  textureScale: vec2f,
  // Blur step direction in source texels.
  delta: vec2f,
  // Scene-referred extraction threshold before exposure.
  threshold: f32,
  // Soft-knee width relative to the threshold.
  softKnee: f32,
  // Camera exposure; the threshold moves with it.
  exposure: f32,
  // Gaussian radius in source texels.
  radius: f32,
  // Contribution of the wider reconstructed glow.
  scatter: f32,
  // Composite glow strength.
  intensity: f32,
  // Starburst brightness.
  starburstIntensity: f32,
  // Starburst ray length in half-resolution texels.
  starburstLength: f32,
};

@group(0) @binding(0) var<uniform> bloom: Bloom;
@group(0) @binding(1) var sourceTexture: texture_2d<f32>;
@group(0) @binding(2) var sourceSampler: sampler;
// Upsample: this level's blurred glow. Composite: the reconstructed glow.
@group(0) @binding(3) var glowTexture: texture_2d<f32>;
// Composite: the starburst streaks.
@group(0) @binding(4) var lensTexture: texture_2d<f32>;

const LUMINANCE = vec3f(0.2126, 0.7152, 0.0722);
const BLOOM_BLUR_MAX_RADIUS = 24.0;
const BLOOM_BLUR_MAX_PAIRS = 12;
// The engine requests two rays (one horizontal streak); luma.gl supports up to eight.
const BLOOM_LENS_SPIKES = 2.0;
const BLOOM_LENS_MAX_DIRECTIONS = 4;
const BLOOM_LENS_STREAK_SAMPLES = 8;

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
 * Output coordinate to source coordinate (luma.gl's `shaderPassRenderer_getTextureUV`).
 */
fn textureUV(coordinate: vec2f) -> vec2f {
  return ((coordinate - vec2f(0.5))/bloom.textureScale) + vec2f(0.5);
}

/**
 * Soft-knee threshold on luminance; the scene-referred threshold is divided by exposure.
 */
fn applyThreshold(sourceColor: vec4f) -> vec4f {
  let luminance = dot(sourceColor.rgb, LUMINANCE);
  let exposure = max(bloom.exposure, 0.0001);
  let threshold = bloom.threshold/exposure;
  let knee = max(threshold*bloom.softKnee, 0.00001);
  let soft = clamp((luminance - threshold + knee)/(2.0*knee), 0.0, 1.0);
  let softContribution = soft*soft*knee;
  let hardContribution = max(luminance - threshold, 0.0);
  return sourceColor*(max(hardContribution, softContribution)/max(luminance, 0.00001));
}

/**
 * One source texel, clamped to the edge.
 */
fn loadColor(coordinate: vec2i) -> vec4f {
  let maximumCoordinate = vec2i(textureDimensions(sourceTexture)) - vec2i(1);
  return textureLoad(sourceTexture, clamp(coordinate, vec2i(0), maximumCoordinate), 0);
}

/**
 * Normalized tent over source texels whose radius follows the source-to-output footprint (at least two texels, the 4x4
 * tent of a half-resolution level). Extraction thresholds each texel before weighting.
 */
fn sampleTent(texCoord: vec2f, extract: bool) -> vec4f {
  let sourceDimensions = vec2i(textureDimensions(sourceTexture));
  let sourceCenter = (texCoord*vec2f(sourceDimensions)) - vec2f(0.5);
  let sourceFootprint = max((abs(dpdx(texCoord)) + abs(dpdy(texCoord)))*vec2f(sourceDimensions), vec2f(1.0));
  let filterRadius = max(sourceFootprint, vec2f(2.0));
  let minimumCoordinate = vec2i(floor(sourceCenter - filterRadius)) + vec2i(1);
  let maximumCoordinate = vec2i(ceil(sourceCenter + filterRadius)) - vec2i(1);
  var color = vec4f(0.0);
  var totalWeight = 0.0;

  for(var sourceY = minimumCoordinate.y; sourceY <= maximumCoordinate.y; sourceY+= 1) {
    let weightY = max(1.0 - (abs(f32(sourceY) - sourceCenter.y)/filterRadius.y), 0.0);
    for(var sourceX = minimumCoordinate.x; sourceX <= maximumCoordinate.x; sourceX+= 1) {
      let weightX = max(1.0 - (abs(f32(sourceX) - sourceCenter.x)/filterRadius.x), 0.0);
      var sourceColor = loadColor(vec2i(sourceX, sourceY));
      if(extract) {
        sourceColor = applyThreshold(sourceColor);
      }
      let weight = weightX*weightY;
      color+= sourceColor*weight;
      totalWeight+= weight;
    }
  }

  return color/max(totalWeight, 0.00001);
}

/**
 * Thresholded tent from the scene into the first (half-resolution) level.
 */
@fragment fn extractMain(input: BloomOutput) -> @location(0) vec4f {
  return sampleTent(textureUV(input.coordinate), true);
}

/**
 * Tent from one level's extraction into the next.
 */
@fragment fn downsampleMain(input: BloomOutput) -> @location(0) vec4f {
  return sampleTent(textureUV(input.coordinate), false);
}

/**
 * Unnormalized Gaussian weight.
 */
fn gaussianWeight(offset: f32, sigma: f32) -> f32 {
  let normalizedOffset = offset/sigma;
  return exp(-0.5*normalizedOffset*normalizedOffset);
}

/**
 * One axis of a Gaussian with sigma radius/3, folding each pair of taps into one linear fetch.
 */
@fragment fn blurMain(input: BloomOutput) -> @location(0) vec4f {
  let texCoord = textureUV(input.coordinate);
  let effectiveRadius = clamp(bloom.radius, 0.0, BLOOM_BLUR_MAX_RADIUS);
  let centerColor = textureSampleLevel(sourceTexture, sourceSampler, texCoord, 0.0);
  if(effectiveRadius <= 0.0) {
    return centerColor;
  }

  let adjustedDelta = bloom.delta/vec2f(textureDimensions(sourceTexture));
  let sigma = max(effectiveRadius/3.0, 0.00001);
  let centerWeight = gaussianWeight(0.0, sigma);
  var color = centerColor*centerWeight;
  var totalWeight = centerWeight;

  for(var pairIndex = 0; pairIndex < BLOOM_BLUR_MAX_PAIRS; pairIndex+= 1) {
    let firstOffset = f32((pairIndex*2) + 1);
    if(firstOffset > effectiveRadius) {
      break;
    }

    let secondOffset = firstOffset + 1.0;
    let firstWeight = gaussianWeight(firstOffset, sigma);
    let secondWeight = select(0.0, gaussianWeight(secondOffset, sigma), secondOffset <= effectiveRadius);
    let combinedWeight = firstWeight + secondWeight;
    let combinedOffset = ((firstOffset*firstWeight) + (secondOffset*secondWeight))/max(combinedWeight, 0.00001);
    let positiveColor = textureSampleLevel(sourceTexture, sourceSampler, texCoord + (adjustedDelta*combinedOffset), 0.0);
    let negativeColor = textureSampleLevel(sourceTexture, sourceSampler, texCoord - (adjustedDelta*combinedOffset), 0.0);
    color+= (positiveColor + negativeColor)*combinedWeight;
    totalWeight+= combinedWeight*2.0;
  }

  return color/totalWeight;
}

/**
 * Cubic B-spline reconstruction of the wider glow from four bilinear fetches.
 */
fn sampleBicubic(texCoord: vec2f) -> vec4f {
  let dimensions = vec2f(textureDimensions(sourceTexture));
  let texelPosition = (texCoord*dimensions) - vec2f(0.5);
  let basePosition = floor(texelPosition);
  let fraction = fract(texelPosition);
  let complement = vec2f(1.0) - fraction;
  let firstWeight = (complement*complement*complement)/6.0;
  let secondWeight = ((fraction*fraction*fraction*3.0) - (fraction*fraction*6.0) + vec2f(4.0))/6.0;
  let thirdWeight = ((-fraction*fraction*fraction*3.0) + (fraction*fraction*3.0) + (fraction*3.0) + vec2f(1.0))/6.0;
  let fourthWeight = (fraction*fraction*fraction)/6.0;
  let firstPairWeight = firstWeight + secondWeight;
  let secondPairWeight = thirdWeight + fourthWeight;
  let firstPosition = (basePosition - vec2f(1.0) + (secondWeight/firstPairWeight) + vec2f(0.5))/dimensions;
  let secondPosition = (basePosition + vec2f(1.0) + (fourthWeight/secondPairWeight) + vec2f(0.5))/dimensions;
  let topLeft = textureSampleLevel(sourceTexture, sourceSampler, firstPosition, 0.0);
  let topRight = textureSampleLevel(sourceTexture, sourceSampler, vec2f(secondPosition.x, firstPosition.y), 0.0);
  let bottomLeft = textureSampleLevel(sourceTexture, sourceSampler, vec2f(firstPosition.x, secondPosition.y), 0.0);
  let bottomRight = textureSampleLevel(sourceTexture, sourceSampler, secondPosition, 0.0);
  return (topLeft*firstPairWeight.x*firstPairWeight.y) + (topRight*secondPairWeight.x*firstPairWeight.y) +
    (bottomLeft*firstPairWeight.x*secondPairWeight.y) + (bottomRight*secondPairWeight.x*secondPairWeight.y);
}

/**
 * Blend this level's blur toward the reconstructed wider glow by the scatter amount.
 */
@fragment fn upsampleMain(input: BloomOutput) -> @location(0) vec4f {
  let texCoord = textureUV(input.coordinate);
  let lowerGlow = sampleBicubic(texCoord);
  let higherGlow = textureSampleLevel(glowTexture, sourceSampler, texCoord, 0.0);
  return mix(higherGlow, lowerGlow, clamp(bloom.scatter, 0.0, 1.0));
}

/**
 * Aperture-diffraction streaks: the reconstructed glow averaged along each ray with exponential falloff.
 */
@fragment fn lensMain(input: BloomOutput) -> @location(0) vec4f {
  let texCoord = textureUV(input.coordinate);
  var lensColor = vec3f(0.0);
  if(bloom.starburstIntensity > 0.0) {
    let directionCount = max(BLOOM_LENS_SPIKES*0.5, 1.0);
    let glowDimensions = vec2f(textureDimensions(sourceTexture));
    var streakColor = vec3f(0.0);
    var totalWeight = 0.0;

    for(var directionIndex = 0; directionIndex < BLOOM_LENS_MAX_DIRECTIONS; directionIndex+= 1) {
      if(f32(directionIndex) >= directionCount) {
        break;
      }

      let angle = (f32(directionIndex)*3.14159265359)/directionCount;
      let direction = vec2f(cos(angle), sin(angle));
      for(var sampleIndex = 1; sampleIndex <= BLOOM_LENS_STREAK_SAMPLES; sampleIndex+= 1) {
        let sampleDistance = f32(sampleIndex)/f32(BLOOM_LENS_STREAK_SAMPLES);
        let offset = (direction*bloom.starburstLength*sampleDistance)/glowDimensions;
        let sampleWeight = exp(-sampleDistance*2.4);
        let positiveColor = textureSampleLevel(sourceTexture, sourceSampler, clamp(texCoord + offset, vec2f(0.0), vec2f(1.0)), 0.0).rgb;
        let negativeColor = textureSampleLevel(sourceTexture, sourceSampler, clamp(texCoord - offset, vec2f(0.0), vec2f(1.0)), 0.0).rgb;
        streakColor+= (positiveColor + negativeColor)*sampleWeight;
        totalWeight+= sampleWeight*2.0;
      }
    }

    lensColor = (streakColor/max(totalWeight, 0.00001))*bloom.starburstIntensity;
  }
  return vec4f(lensColor, 1.0);
}

/**
 * Add glow and streaks to the scene. Used when a later blur reads the bloomed scene; otherwise the display resolve
 * applies the same sum.
 */
@fragment fn compositeMain(input: BloomOutput) -> @location(0) vec4f {
  let texCoord = textureUV(input.coordinate);
  let sourceColor = textureSampleLevel(sourceTexture, sourceSampler, texCoord, 0.0);
  let glowColor = textureSampleLevel(glowTexture, sourceSampler, texCoord, 0.0).rgb;
  let lensColor = textureSampleLevel(lensTexture, sourceSampler, texCoord, 0.0).rgb;
  return vec4f(sourceColor.rgb + ((glowColor + lensColor)*bloom.intensity), sourceColor.a);
}
