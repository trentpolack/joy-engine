// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/screen-space-global-illumination.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

struct SSGITemporalUniforms {
  inverseProjectionMatrix: mat4x4f,
  historyWeight: f32,
  depthThreshold: f32,
};

@group(0) @binding(auto) var<uniform> ssgiTemporal: SSGITemporalUniforms;
@group(0) @binding(auto) var historyTexture: texture_2d<f32>;
@group(0) @binding(auto) var historyTextureSampler: sampler;
@group(0) @binding(auto) var velocityTexture: texture_2d<f32>;
@group(0) @binding(auto) var velocityTextureSampler: sampler;
@group(0) @binding(auto) var depthTexture: texture_depth_2d;
@group(0) @binding(auto) var depthTextureSampler: sampler;
@group(0) @binding(auto) var previousDepthTexture: texture_2d<f32>;
@group(0) @binding(auto) var previousDepthTextureSampler: sampler;

fn ssgiTemporal_reconstructViewDepth(texCoord: vec2f, depth: f32) -> f32 {
  let clip = vec4f(texCoord.x * 2.0 - 1.0, 1.0 - texCoord.y * 2.0, depth, 1.0);
  let viewPosition = ssgiTemporal.inverseProjectionMatrix * clip;
  return abs(viewPosition.z / max(abs(viewPosition.w), 0.00001));
}

fn ssgiTemporal_sampleColor(
  sourceTexture: texture_2d<f32>,
  sourceTextureSampler: sampler,
  texSize: vec2f,
  texCoord: vec2f
) -> vec4f {
  let current = textureSampleLevel(sourceTexture, sourceTextureSampler, texCoord, 0);
  let currentDepth = textureSampleLevel(depthTexture, depthTextureSampler, texCoord, 0);
  if (currentDepth >= 0.99999) {
    return vec4f(0.0);
  }

  let velocity = textureSampleLevel(velocityTexture, velocityTextureSampler, texCoord, 0).xy;
  let previousCoord = texCoord - velocity;
  let validCoordinate = all(previousCoord >= vec2f(0.0)) &&
    all(previousCoord <= vec2f(1.0));
  let clampedPreviousCoord = clamp(previousCoord, vec2f(0.0), vec2f(1.0));
  let previousDepth = textureSampleLevel(
    previousDepthTexture,
    previousDepthTextureSampler,
    clampedPreviousCoord,
    0
  ).r;
  let currentViewDepth = ssgiTemporal_reconstructViewDepth(texCoord, currentDepth);
  let previousViewDepth = ssgiTemporal_reconstructViewDepth(clampedPreviousCoord, previousDepth);
  let relativeDepthDifference = abs(previousViewDepth - currentViewDepth) /
    max(currentViewDepth, 0.0001);
  let validDepth = relativeDepthDifference < ssgiTemporal.depthThreshold;

  let texel = 1.0 / vec2f(textureDimensions(sourceTexture));
  var minimumRadiance = current;
  var maximumRadiance = current;
  for (var sampleY: i32 = -1; sampleY <= 1; sampleY++) {
    for (var sampleX: i32 = -1; sampleX <= 1; sampleX++) {
      let sampleCoord = clamp(
        texCoord + vec2f(f32(sampleX), f32(sampleY)) * texel,
        vec2f(0.0),
        vec2f(1.0)
      );
      let neighborhoodRadiance = textureSampleLevel(
        sourceTexture,
        sourceTextureSampler,
        sampleCoord,
        0
      );
      minimumRadiance = min(minimumRadiance, neighborhoodRadiance);
      maximumRadiance = max(maximumRadiance, neighborhoodRadiance);
    }
  }

  let historyRadiance = clamp(
    textureSampleLevel(historyTexture, historyTextureSampler, clampedPreviousCoord, 0),
    minimumRadiance,
    maximumRadiance
  );
  let historyWeight = select(
    0.0,
    ssgiTemporal.historyWeight,
    validCoordinate && validDepth
  );
  return mix(current, historyRadiance, historyWeight);
}
