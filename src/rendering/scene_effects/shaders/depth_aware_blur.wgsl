// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/depth-aware-blur.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

struct depthAwareBlurUniforms {
  direction: vec2f,
  radius: f32,
  depthSigma: f32,
  spatialSigma: f32,
};

@group(0) @binding(auto) var<uniform> depthAwareBlur: depthAwareBlurUniforms;
@group(0) @binding(auto) var depthTexture: texture_depth_2d;
@group(0) @binding(auto) var depthTextureSampler: sampler;

fn depthAwareBlur_sampleColor(
  sourceTexture: texture_2d<f32>,
  sourceTextureSampler: sampler,
  texSize: vec2f,
  texCoord: vec2f
) -> vec4f {
  let sourceDimensions = vec2f(textureDimensions(sourceTexture));
  let texel = depthAwareBlur.direction / sourceDimensions;
  let centerDepth = textureSample(depthTexture, depthTextureSampler, texCoord);
  var color = textureSample(sourceTexture, sourceTextureSampler, texCoord);
  var totalWeight = 1.0;
  for (var index: i32 = 1; index <= 8; index++) {
    if (f32(index) > depthAwareBlur.radius) { break; }
    let offset = texel * f32(index);
    for (var side: i32 = -1; side <= 1; side += 2) {
      let sampleUv = clamp(texCoord + offset * f32(side), vec2f(0.0), vec2f(1.0));
      let sampleDepth = textureSample(depthTexture, depthTextureSampler, sampleUv);
      let spatialWeight = exp(-f32(index * index) / max(2.0 * depthAwareBlur.spatialSigma * depthAwareBlur.spatialSigma, 0.0001));
      let depthDelta = abs(sampleDepth - centerDepth);
      let depthWeight = exp(-(depthDelta * depthDelta) / max(2.0 * depthAwareBlur.depthSigma * depthAwareBlur.depthSigma, 0.000001));
      let weight = spatialWeight * depthWeight;
      color += textureSample(sourceTexture, sourceTextureSampler, sampleUv) * weight;
      totalWeight += weight;
    }
  }
  return color / totalWeight;
}
