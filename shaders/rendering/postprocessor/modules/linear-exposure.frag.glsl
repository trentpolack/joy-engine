// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

layout(std140) uniform linearExposureUniforms {
  float exposure;
} linearExposure;

/**
 * Scales linear scene color by the configured photographic exposure.
 * Negative RGB values are clamped after scaling, while alpha bypasses exposure because it represents coverage rather than emitted light.
 */
vec4 linearExposure_filterColor_ext(vec4 color, vec2 texSize, vec2 texCoord) {
  return(vec4(max(color.rgb * linearExposure.exposure, vec3(0.0)), color.a));
}
