// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

layout(std140) uniform antialiasUniforms {
  float amount;
} antialias;

/**
 * Blends the source sample with the shared FXAA result.
 * The configurable amount preserves source alpha and allows callers to reduce edge smoothing without changing the FXAA implementation.
 */
vec4 antialias_sampleColor(sampler2D sourceTexture, vec2 texSize, vec2 texCoord) {
  vec4 sourceColor = texture(sourceTexture, texCoord);
  vec4 antialiasedColor = fxaa_sampleColor(sourceTexture, texSize, texCoord);

  return(vec4(mix(sourceColor.rgb, antialiasedColor.rgb, antialias.amount), sourceColor.a));
}
