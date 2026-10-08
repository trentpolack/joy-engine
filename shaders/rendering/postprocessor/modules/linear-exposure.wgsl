// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

struct linearExposureUniforms {
  exposure: f32,
};

@group(0) @binding(auto) var<uniform> linearExposure: linearExposureUniforms;

/**
 * Filters a color using linear exposure.
 * @param color The input color.
 * @param texSize The size of the texture.
 * @param texCoord The texture coordinates.
 * @returns The filtered color.
 */
fn linearExposure_filterColor_ext(
  color: vec4f,
  texSize: vec2f,
  texCoord: vec2f
  ) -> vec4f {
  return(vec4f(max(color.rgb*linearExposure.exposure, vec3f(0.0)), color.a));
}
