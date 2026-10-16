// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/motion-blur.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

struct motionBlurUniforms {
  strength: f32,
  sampleCount: f32,
};
@group(0) @binding(auto) var<uniform> motionBlur: motionBlurUniforms;
@group(0) @binding(auto) var velocityTexture: texture_2d<f32>;
@group(0) @binding(auto) var velocityTextureSampler: sampler;
@group(0) @binding(auto) var depthTexture: texture_depth_2d;
@group(0) @binding(auto) var depthTextureSampler: sampler;
fn motionBlur_sampleColor(
  sourceTexture: texture_2d<f32>, sourceTextureSampler: sampler, texSize: vec2f, texCoord: vec2f
) -> vec4f {
  let sceneCoord = texCoord;
  let velocity = textureSample(velocityTexture, velocityTextureSampler, sceneCoord).xy * motionBlur.strength;
  let centerDepth = textureSample(depthTexture, depthTextureSampler, sceneCoord);
  var color = vec4f(0.0);
  var totalWeight = 0.0;
  for (var index: i32 = 0; index < 16; index++) {
    if (f32(index) >= motionBlur.sampleCount) { break; }
    let amount = (f32(index) / max(motionBlur.sampleCount - 1.0, 1.0)) - 0.5;
    let sampleUv = clamp(texCoord - velocity * amount, vec2f(0.0), vec2f(1.0));
    let sampleDepth = textureSample(depthTexture, depthTextureSampler, sampleUv);
    let weight = exp(-abs(sampleDepth - centerDepth) * 240.0);
    color += textureSample(sourceTexture, sourceTextureSampler, sampleUv) * weight;
    totalWeight += weight;
  }
  return color / max(totalWeight, 0.0001);
}
