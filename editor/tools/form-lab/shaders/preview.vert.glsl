// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Match preview.wgsl: 80 caller-owned bytes followed by Joy Engine's uniform tail.
layout(std140) uniform uniforms {
  mat4 viewProjection;
  vec4 tint;
  // JOY_POST_UNIFORMS_MACRO
};

layout(location = 0) in vec3 position;
layout(location = 1) in vec4 color;

out vec4 vertexColor;

/**
 * Projects authored FORM geometry for the preview viewport.
 * FORM positions already use world space, so this stage applies only the camera transform and preview tint.
 */
void main() {
  // FORM output is already in world space, so the preview needs no model matrix.
  gl_Position = viewProjection * vec4(position, 1.0);

  // Tint before interpolation to match the WebGPU preview path exactly.
  vertexColor = color*tint;
}
