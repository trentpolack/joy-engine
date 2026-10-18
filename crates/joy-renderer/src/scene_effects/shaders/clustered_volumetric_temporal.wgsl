// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/clustered-volumetric-lighting.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

struct ClusteredVolumetricTemporalUniforms {
  inverseProjectionMatrix: mat4x4f,
  inverseViewProjectionMatrix: mat4x4f,
  previousViewProjectionMatrix: mat4x4f,
  historyWeight: f32,
  depthThreshold: f32,
};

@group(0) @binding(auto) var<uniform> clusteredVolumetricTemporal:
  ClusteredVolumetricTemporalUniforms;
@group(0) @binding(auto) var historyTexture: texture_2d<f32>;
@group(0) @binding(auto) var historyTextureSampler: sampler;
@group(0) @binding(auto) var velocityTexture: texture_2d<f32>;
@group(0) @binding(auto) var velocityTextureSampler: sampler;
@group(0) @binding(auto) var depthTexture: texture_depth_2d;
@group(0) @binding(auto) var depthTextureSampler: sampler;
@group(0) @binding(auto) var previousDepthTexture: texture_2d<f32>;

fn clusteredVolumetricTemporal_viewDepth(texCoord: vec2f, depth: f32) -> f32 {
  let clip = vec4f(texCoord.x * 2.0 - 1.0, 1.0 - texCoord.y * 2.0, depth, 1.0);
  let viewPosition = clusteredVolumetricTemporal.inverseProjectionMatrix * clip;
  return abs(viewPosition.z / max(abs(viewPosition.w), 0.00001));
}

fn clusteredVolumetricTemporal_cameraPreviousCoord(texCoord: vec2f, depth: f32) -> vec3f {
  let clip = vec4f(texCoord.x * 2.0 - 1.0, 1.0 - texCoord.y * 2.0, depth, 1.0);
  let worldPosition = clusteredVolumetricTemporal.inverseViewProjectionMatrix * clip;
  let world = worldPosition.xyz / max(abs(worldPosition.w), 0.00001);
  let previousClip = clusteredVolumetricTemporal.previousViewProjectionMatrix * vec4f(world, 1.0);
  let previousNdc = previousClip.xy / max(abs(previousClip.w), 0.00001);
  let previousCoord = previousNdc * vec2f(0.5, -0.5) + vec2f(0.5);
  return vec3f(previousCoord, select(0.0, 1.0, previousClip.w > 0.00001));
}

fn clusteredVolumetricTemporal_sampleColor(
  sourceTexture: texture_2d<f32>,
  sourceTextureSampler: sampler,
  texSize: vec2f,
  texCoord: vec2f
) -> vec4f {
  let current = textureSampleLevel(sourceTexture, sourceTextureSampler, texCoord, 0);
  let currentDepth = textureSampleLevel(depthTexture, depthTextureSampler, texCoord, 0);
  let velocity = textureSampleLevel(velocityTexture, velocityTextureSampler, texCoord, 0).xy;
  let cameraReprojection = clusteredVolumetricTemporal_cameraPreviousCoord(texCoord, currentDepth);
  let velocityIsFinite = all(velocity == velocity) && all(abs(velocity) <= vec2f(1.0));
  let useObjectVelocity = currentDepth < 0.99999 && velocityIsFinite;
  let previousCoord = select(cameraReprojection.xy, texCoord - velocity, useObjectVelocity);
  let validCoordinate = all(previousCoord >= vec2f(0.0)) &&
    all(previousCoord <= vec2f(1.0)) &&
    (useObjectVelocity || cameraReprojection.z > 0.5);
  let clampedPreviousCoord = clamp(previousCoord, vec2f(0.0), vec2f(1.0));
  let previousDepthDimensions = textureDimensions(previousDepthTexture);
  let previousDepthCoordinate = min(
    vec2u(clampedPreviousCoord * vec2f(previousDepthDimensions)),
    previousDepthDimensions - vec2u(1u)
  );
  let currentViewDepth = clusteredVolumetricTemporal_viewDepth(texCoord, currentDepth);
  let previousViewDepth = textureLoad(
    previousDepthTexture,
    vec2i(previousDepthCoordinate),
    0
  ).r;
  let relativeDepthDifference = abs(previousViewDepth - currentViewDepth) /
    max(currentViewDepth, 0.0001);
  let validDepth = previousViewDepth > 0.0 &&
    relativeDepthDifference < clusteredVolumetricTemporal.depthThreshold;

  let texel = 1.0 / vec2f(textureDimensions(sourceTexture));
  var minimumScattering = current;
  var maximumScattering = current;
  for (var sampleY: i32 = -1; sampleY <= 1; sampleY++) {
    for (var sampleX: i32 = -1; sampleX <= 1; sampleX++) {
      let sampleCoord = clamp(
        texCoord + vec2f(f32(sampleX), f32(sampleY)) * texel,
        vec2f(0.0),
        vec2f(1.0)
      );
      let sampleScattering = textureSampleLevel(
        sourceTexture,
        sourceTextureSampler,
        sampleCoord,
        0
      );
      minimumScattering = min(minimumScattering, sampleScattering);
      maximumScattering = max(maximumScattering, sampleScattering);
    }
  }

  let history = clamp(
    textureSampleLevel(historyTexture, historyTextureSampler, clampedPreviousCoord, 0),
    minimumScattering,
    maximumScattering
  );
  let weight = select(
    0.0,
    clusteredVolumetricTemporal.historyWeight,
    validCoordinate && validDepth
  );
  return mix(current, history, weight);
}
