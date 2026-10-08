// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

struct antialiasUniforms {
  amount: f32,
};

@group(0) @binding(auto) var<uniform> antialias: antialiasUniforms;

/**
 * Samples a color with antialiasing effect.
 * @param sourceTexture The source texture.
 * @param sourceTextureSampler The source texture sampler.
 * @param texSize The size of the texture.
 * @param texCoord The texture coordinates.
 * @returns The antialiased color.
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
