// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/screen-space-global-illumination.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

struct SSGICompositeUniforms {
  strength: f32,
  debugMode: f32,
};

@group(0) @binding(auto) var<uniform> ssgiComposite: SSGICompositeUniforms;
@group(0) @binding(auto) var indirectLightingTexture: texture_2d<f32>;
@group(0) @binding(auto) var indirectLightingTextureSampler: sampler;

fn ssgiComposite_sampleColor(
  sourceTexture: texture_2d<f32>,
  sourceTextureSampler: sampler,
  texSize: vec2f,
  texCoord: vec2f
) -> vec4f {
  let color = textureSampleLevel(sourceTexture, sourceTextureSampler, texCoord, 0);
  let indirect = textureSampleLevel(
    indirectLightingTexture,
    indirectLightingTextureSampler,
    texCoord,
    0
  );
  if (ssgiComposite.debugMode > 1.5) {
    let confidence = clamp(indirect.a, 0.0, 1.0);
    return vec4f(
      mix(vec3f(0.025, 0.05, 0.18), vec3f(1.0, 0.72, 0.18), confidence),
      1.0
    );
  }
  if (ssgiComposite.debugMode > 0.5) {
    return vec4f(indirect.rgb * 2.2, 1.0);
  }
  return vec4f(color.rgb + indirect.rgb * ssgiComposite.strength, color.a);
}
