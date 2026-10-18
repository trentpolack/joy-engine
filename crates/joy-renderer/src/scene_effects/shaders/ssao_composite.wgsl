// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/ssao.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

struct ssaoCompositeUniforms {
  debugMode: f32,
};
@group(0) @binding(auto) var<uniform> ssaoComposite: ssaoCompositeUniforms;
@group(0) @binding(auto) var ambientOcclusionTexture: texture_2d<f32>;
@group(0) @binding(auto) var ambientOcclusionTextureSampler: sampler;
fn ssaoComposite_sampleColor(
  sourceTexture: texture_2d<f32>, sourceTextureSampler: sampler, texSize: vec2f, texCoord: vec2f
) -> vec4f {
  let color = textureSample(sourceTexture, sourceTextureSampler, texCoord);
  let ambient = textureSample(ambientOcclusionTexture, ambientOcclusionTextureSampler, texCoord).r;
  if (ssaoComposite.debugMode > 0.5) { return vec4f(vec3f(ambient), 1.0); }
  let rawOcclusion = clamp(1.0 - ambient, 0.0, 1.0);
  let contactOcclusion = smoothstep(0.03, 0.5, rawOcclusion);
  return vec4f(color.rgb * (1.0 - contactOcclusion * 0.48), color.a);
}
