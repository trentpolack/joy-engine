// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/clustered-volumetric-lighting.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

struct ClusteredVolumetricCompositeUniforms {
  strength: f32,
  debugMode: f32,
};

@group(0) @binding(auto) var<uniform> clusteredVolumetricComposite:
  ClusteredVolumetricCompositeUniforms;
@group(0) @binding(auto) var scatteringTexture: texture_2d<f32>;
@group(0) @binding(auto) var scatteringTextureSampler: sampler;

fn clusteredVolumetricComposite_sampleColor(
  sourceTexture: texture_2d<f32>,
  sourceTextureSampler: sampler,
  texSize: vec2f,
  texCoord: vec2f
) -> vec4f {
  let source = textureSampleLevel(sourceTexture, sourceTextureSampler, texCoord, 0);
  let scattering = textureSampleLevel(scatteringTexture, scatteringTextureSampler, texCoord, 0);
  if (clusteredVolumetricComposite.debugMode > 1.5) {
    return vec4f(vec3f(clamp(scattering.a, 0.0, 1.0)), 1.0);
  }
  if (clusteredVolumetricComposite.debugMode > 0.5) {
    return vec4f(scattering.rgb * 2.4, 1.0);
  }
  let strength = clamp(clusteredVolumetricComposite.strength, 0.0, 2.0);
  let transmittance = mix(1.0, clamp(scattering.a, 0.0, 1.0), min(strength, 1.0));
  return vec4f(source.rgb * transmittance + scattering.rgb * strength, source.a);
}
