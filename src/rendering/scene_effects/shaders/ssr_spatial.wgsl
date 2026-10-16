// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/screen-space-reflections.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

struct SSRSpatialUniforms {
  inverseProjectionMatrix: mat4x4f,
  direction: vec2f,
  maxRadius: f32,
  depthSigma: f32,
};

@group(0) @binding(auto) var<uniform> ssrSpatial: SSRSpatialUniforms;
@group(0) @binding(auto) var depthTexture: texture_depth_2d;
@group(0) @binding(auto) var depthTextureSampler: sampler;
@group(0) @binding(auto) var normalTexture: texture_2d<f32>;
@group(0) @binding(auto) var normalTextureSampler: sampler;

fn ssrSpatial_reconstructViewDepth(texCoord: vec2f, depth: f32) -> f32 {
  let clip = vec4f(texCoord.x * 2.0 - 1.0, 1.0 - texCoord.y * 2.0, depth, 1.0);
  let viewPosition = ssrSpatial.inverseProjectionMatrix * clip;
  return abs(viewPosition.z / max(abs(viewPosition.w), 0.00001));
}

fn ssrSpatial_sampleColor(
  sourceTexture: texture_2d<f32>,
  sourceTextureSampler: sampler,
  texSize: vec2f,
  texCoord: vec2f
) -> vec4f {
  let centerReflection = textureSampleLevel(sourceTexture, sourceTextureSampler, texCoord, 0);
  let centerNormalRoughness = textureSampleLevel(normalTexture, normalTextureSampler, texCoord, 0);
  let roughness = clamp(centerNormalRoughness.a, 0.0, 1.0);
  let centerDepth = textureSampleLevel(depthTexture, depthTextureSampler, texCoord, 0);
  if (centerDepth >= 0.99999) {
    return vec4f(0.0);
  }
  let centerViewDepth = ssrSpatial_reconstructViewDepth(texCoord, centerDepth);
  let hasCenterReflection = centerReflection.a > 0.001;
  if (!hasCenterReflection && roughness >= 0.28) {
    return centerReflection;
  }
  let minimumRadius = select(5.0, 0.95, hasCenterReflection);
  let radius = clamp(
    max(roughness * roughness * ssrSpatial.maxRadius, minimumRadius),
    minimumRadius,
    6.0
  );

  let centerNormal = normalize(centerNormalRoughness.rgb * 2.0 - 1.0);
  let texel = ssrSpatial.direction / vec2f(textureDimensions(sourceTexture));
  let centerWeight = select(0.0, 1.0, hasCenterReflection);
  var reflection = centerReflection * centerWeight;
  var totalWeight = centerWeight;
  var supportingSampleCount = 0u;
  for (var sampleIndex: i32 = 1; sampleIndex <= 6; sampleIndex++) {
    if (f32(sampleIndex) > ceil(radius)) {
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
      let sampleViewDepth = ssrSpatial_reconstructViewDepth(sampleCoord, sampleDepth);
      let relativeDepthDelta = abs(sampleViewDepth - centerViewDepth) /
        max(centerViewDepth, 0.0001);
      let depthWeight = exp(
        -(relativeDepthDelta * relativeDepthDelta) /
          max(2.0 * ssrSpatial.depthSigma * ssrSpatial.depthSigma, 0.000001)
      );
      let normalWeight = pow(max(dot(centerNormal, sampleNormal), 0.0), 16.0);
      let distanceWeight = exp(
        -f32(sampleIndex * sampleIndex) / max(2.0 * radius * radius, 0.1)
      );
      let sampleReflection = textureSampleLevel(sourceTexture, sourceTextureSampler, sampleCoord, 0);
      let confidenceWeight = smoothstep(0.001, 0.12, sampleReflection.a);
      let belongsToSamePlane = dot(centerNormal, sampleNormal) > 0.9995;
      let surfaceWeight = select(0.0, 1.0, hasCenterReflection || belongsToSamePlane);
      let weight = depthWeight * normalWeight * distanceWeight * confidenceWeight * surfaceWeight;
      if (weight > 0.001) {
        supportingSampleCount += 1u;
      }
      reflection += sampleReflection * weight;
      totalWeight += weight;
    }
  }

  if ((!hasCenterReflection && supportingSampleCount < 2u) || totalWeight <= 0.0001) {
    return centerReflection;
  }
  return reflection / totalWeight;
}
