// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

layout(std140) uniform linearExposureUniforms {
  float exposure;
} linearExposure;

/**
 * Filters a color using linear exposure.
 * @param color The input color.
 * @param texSize The size of the texture.
 * @param texCoord The texture coordinates.
 * @returns The filtered color.
 */
vec4 linearExposure_filterColor_ext(vec4 color, vec2 texSize, vec2 texCoord) {
  return(vec4(max(color.rgb * linearExposure.exposure, vec3(0.0)), color.a));
}
