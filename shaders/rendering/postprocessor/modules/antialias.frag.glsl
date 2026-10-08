// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

layout(std140) uniform antialiasUniforms {
  float amount;
} antialias;

/**
 * Samples a color with antialiasing effect.
 * @param sourceTexture The source texture.
 * @param sourceTextureSampler The source texture sampler.
 * @param texSize The size of the texture.
 * @param texCoord The texture coordinates.
 * @returns The antialiased color.
 */
vec4 antialias_sampleColor(
  sampler2D sourceTexture,
  vec2 texSize,
  vec2 texCoord
  ) {
  vec4 sourceColor = texture(sourceTexture, texCoord);
  vec4 antialiasedColor = fxaa_sampleColor(sourceTexture, texSize, texCoord);

  return(vec4(mix(sourceColor.rgb, antialiasedColor.rgb, antialias.amount), sourceColor.a));
}
