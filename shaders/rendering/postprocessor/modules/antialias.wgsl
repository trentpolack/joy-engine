// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

struct antialiasUniforms {
  amount: f32,
};

@group(0) @binding(auto) var<uniform> antialias: antialiasUniforms;

/**
 * Blends the source sample with the shared FXAA result.
 * The configurable amount preserves source alpha and allows callers to reduce edge smoothing without changing the FXAA implementation.
 */
fn antialias_sampleColor(
  sourceTexture: texture_2d<f32>,
  sourceTextureSampler: sampler,
  texSize: vec2f,
  texCoord: vec2f
) -> vec4f {
  let sourceColor = textureSample(sourceTexture, sourceTextureSampler, texCoord);
  let antialiasedColor = fxaa_sampleColor(
    sourceTexture,
    sourceTextureSampler,
    texSize,
    texCoord
  );

  return(vec4f(mix(sourceColor.rgb, antialiasedColor.rgb, antialias.amount), sourceColor.a));
}
