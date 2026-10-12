// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

layout(std140) uniform vignetteBlurUniforms {
  float amount;
  float radius;
} vignetteBlur;

uniform sampler2D blurredTexture;

/**
 * Blends a pre-blurred image into the source near the viewport boundary.
 * Aspect-corrected distance keeps the vignette circular on non-square targets, and source alpha remains authoritative for compositing.
 *  TODO (trent, 10/12/26): Implement proper vignette blur logic and not a texture sampled one.
 */
vec4 vignetteBlur_sampleColor(
  sampler2D sourceTexture,
  vec2 texSize,
  vec2 texCoord
  ) {
  vec2 aspectScale = texSize/max(min(texSize.x, texSize.y), 1.0);
  float edgeDistance = length((texCoord - vec2(0.5))*aspectScale);
  float edgeStart = vignetteBlur.radius;
  float edgeEnd = max(0.82, edgeStart + 0.01);
  float edgeWeight = smoothstep(edgeStart, edgeEnd, edgeDistance)*vignetteBlur.amount;
  vec4 sourceColor = texture(sourceTexture, texCoord);
  vec3 blurredColor = texture(blurredTexture, texCoord).rgb;

  return(vec4(mix(sourceColor.rgb, blurredColor, edgeWeight), sourceColor.a));
}
