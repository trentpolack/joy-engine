// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/screen-space-global-illumination.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

struct SSGISpatialUniforms {
  direction: vec2f,
  radius: f32,
  depthSigma: f32,
};

@group(0) @binding(auto) var<uniform> ssgiSpatial: SSGISpatialUniforms;
@group(0) @binding(auto) var depthTexture: texture_depth_2d;
@group(0) @binding(auto) var depthTextureSampler: sampler;
@group(0) @binding(auto) var normalTexture: texture_2d<f32>;
@group(0) @binding(auto) var normalTextureSampler: sampler;

fn ssgiSpatial_sampleColor(
  sourceTexture: texture_2d<f32>,
  sourceTextureSampler: sampler,
  texSize: vec2f,
  texCoord: vec2f
) -> vec4f {
  let centerRadiance = textureSampleLevel(sourceTexture, sourceTextureSampler, texCoord, 0);
  let centerDepth = textureSampleLevel(depthTexture, depthTextureSampler, texCoord, 0);
  if (centerDepth >= 0.99999) {
    return vec4f(0.0);
  }

  let centerNormal = normalize(
    textureSampleLevel(normalTexture, normalTextureSampler, texCoord, 0).rgb * 2.0 - 1.0
  );
  let texel = ssgiSpatial.direction / vec2f(textureDimensions(sourceTexture));
  var radiance = centerRadiance;
  var accumulatedWeight = 1.0;
  for (var sampleIndex: i32 = 1; sampleIndex <= 5; sampleIndex++) {
    if (f32(sampleIndex) > ssgiSpatial.radius) {
      break;
    }
    for (var side: i32 = -1; side <= 1; side += 2) {
      let sampleCoord = clamp(
        texCoord + texel * f32(sampleIndex * side),
        vec2f(0.0),
        vec2f(1.0)
      );
      let sampleDepth = textureSampleLevel(depthTexture, depthTextureSampler, sampleCoord, 0);
      let sampleNormal = normalize(
        textureSampleLevel(normalTexture, normalTextureSampler, sampleCoord, 0).rgb * 2.0 - 1.0
      );
      let depthDelta = abs(sampleDepth - centerDepth);
      let depthWeight = exp(
        -(depthDelta * depthDelta) /
          max(2.0 * ssgiSpatial.depthSigma * ssgiSpatial.depthSigma, 0.000001)
      );
      let normalWeight = pow(max(dot(centerNormal, sampleNormal), 0.0), 12.0);
      let distanceWeight = exp(
        -f32(sampleIndex * sampleIndex) /
          max(2.0 * ssgiSpatial.radius * ssgiSpatial.radius, 0.1)
      );
      let weight = depthWeight * normalWeight * distanceWeight;
      radiance += textureSampleLevel(sourceTexture, sourceTextureSampler, sampleCoord, 0) * weight;
      accumulatedWeight += weight;
    }
  }

  return radiance / max(accumulatedWeight, 0.0001);
}
