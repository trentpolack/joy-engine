// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Depth-only directional shadow caster. Instances use the same vertex and instance layout as the surface materials so
// caster geometry is always identical to what the camera sees.

struct Caster {
  lightViewProjection: mat4x4f,
};

@group(0) @binding(0) var<uniform> caster: Caster;

/**
 * Project an instanced mesh vertex into the light's orthographic volume. Only the position attribute is read.
 */
@vertex fn vertexMain(
  @location(0) position: vec3f,
  @location(5) model0: vec4f,
  @location(6) model1: vec4f,
  @location(7) model2: vec4f,
  @location(8) model3: vec4f
) -> @builtin(position) vec4f {
  let model = mat4x4f(model0, model1, model2, model3);
  return caster.lightViewProjection*(model*vec4f(position, 1.0));
}
