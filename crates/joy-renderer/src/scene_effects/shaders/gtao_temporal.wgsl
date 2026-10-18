// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/gtao.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

struct GTAOTemporalUniforms {
  inverseProjectionMatrix: mat4x4f,
  historyWeight: f32,
  depthThreshold: f32,
};

@group(0) @binding(auto) var<uniform> gtaoTemporal: GTAOTemporalUniforms;
@group(0) @binding(auto) var historyTexture: texture_2d<f32>;
@group(0) @binding(auto) var historyTextureSampler: sampler;
@group(0) @binding(auto) var velocityTexture: texture_2d<f32>;
@group(0) @binding(auto) var velocityTextureSampler: sampler;
@group(0) @binding(auto) var depthTexture: texture_depth_2d;
@group(0) @binding(auto) var depthTextureSampler: sampler;
@group(0) @binding(auto) var previousDepthTexture: texture_2d<f32>;
@group(0) @binding(auto) var previousDepthTextureSampler: sampler;

fn gtaoTemporal_reconstructViewDepth(texCoord: vec2f, depth: f32) -> f32 {
  let clip = vec4f(texCoord.x * 2.0 - 1.0, 1.0 - texCoord.y * 2.0, depth, 1.0);
  let viewPosition = gtaoTemporal.inverseProjectionMatrix * clip;
  return abs(viewPosition.z / max(abs(viewPosition.w), 0.00001));
}

fn gtaoTemporal_sampleColor(
  sourceTexture: texture_2d<f32>,
  sourceTextureSampler: sampler,
  texSize: vec2f,
  texCoord: vec2f
) -> vec4f {
  let current = textureSample(sourceTexture, sourceTextureSampler, texCoord);
  let velocity = textureSample(velocityTexture, velocityTextureSampler, texCoord).xy;
  let previousCoord = texCoord - velocity;
  let validCoord = all(previousCoord >= vec2f(0.0)) && all(previousCoord <= vec2f(1.0));
  let clampedPreviousCoord = clamp(previousCoord, vec2f(0.0), vec2f(1.0));
  let currentDepth = textureSample(depthTexture, depthTextureSampler, texCoord);
  let previousDepth = textureSample(
    previousDepthTexture,
    previousDepthTextureSampler,
    clampedPreviousCoord
  ).r;
  let currentViewDepth = gtaoTemporal_reconstructViewDepth(texCoord, currentDepth);
  let previousViewDepth = gtaoTemporal_reconstructViewDepth(
    clampedPreviousCoord,
    previousDepth
  );
  let relativeDepthDifference = abs(previousViewDepth - currentViewDepth) /
    max(currentViewDepth, 0.0001);
  let validDepth = relativeDepthDifference < gtaoTemporal.depthThreshold;

  let texel = 1.0 / vec2f(textureDimensions(sourceTexture));
  var minimumVisibility = current.r;
  var maximumVisibility = current.r;
  for (var y: i32 = -1; y <= 1; y++) {
    for (var x: i32 = -1; x <= 1; x++) {
      let sampleVisibility = textureSample(
        sourceTexture,
        sourceTextureSampler,
        texCoord + vec2f(f32(x), f32(y)) * texel
      ).r;
      minimumVisibility = min(minimumVisibility, sampleVisibility);
      maximumVisibility = max(maximumVisibility, sampleVisibility);
    }
  }
  let historyVisibility = clamp(
    textureSample(historyTexture, historyTextureSampler, clampedPreviousCoord).r,
    minimumVisibility,
    maximumVisibility
  );
  let historyWeight = select(0.0, gtaoTemporal.historyWeight, validCoord && validDepth);
  let visibility = mix(current.r, historyVisibility, historyWeight);
  return vec4f(vec3f(visibility), 1.0);
}
