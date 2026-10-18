// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/gtao.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

struct GTAOCompositeUniforms {
  strength: f32,
  debugMode: f32,
};

@group(0) @binding(auto) var<uniform> gtaoComposite: GTAOCompositeUniforms;
@group(0) @binding(auto) var ambientOcclusionTexture: texture_2d<f32>;
@group(0) @binding(auto) var ambientOcclusionTextureSampler: sampler;

fn gtaoComposite_sampleColor(
  sourceTexture: texture_2d<f32>,
  sourceTextureSampler: sampler,
  texSize: vec2f,
  texCoord: vec2f
) -> vec4f {
  let color = textureSample(sourceTexture, sourceTextureSampler, texCoord);
  let visibility = textureSample(
    ambientOcclusionTexture,
    ambientOcclusionTextureSampler,
    texCoord
  ).r;
  if (gtaoComposite.debugMode > 0.5) {
    return vec4f(vec3f(visibility), 1.0);
  }
  let ambientVisibility = mix(1.0, visibility, clamp(gtaoComposite.strength, 0.0, 1.0));
  return vec4f(color.rgb * ambientVisibility, color.a);
}
