// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

struct linearExposureUniforms {
  exposure: f32,
};

@group(0) @binding(auto) var<uniform> linearExposure: linearExposureUniforms;

/**
 * Scales linear scene color by the configured photographic exposure.
 * Negative RGB values are clamped after scaling, while alpha bypasses exposure because it represents coverage rather than emitted light.
 */
fn linearExposure_filterColor_ext(
  color: vec4f,
  texSize: vec2f,
  texCoord: vec2f
  ) -> vec4f {
  return(vec4f(max(color.rgb*linearExposure.exposure, vec3f(0.0)), color.a));
}
