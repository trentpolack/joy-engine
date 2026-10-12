// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

precision highp float;

layout(std140) uniform uniforms {
  mat4 viewProjection;
  vec4 tint;
  // JOY_POST_UNIFORMS_MACRO
};

in vec4 vertexColor;
out vec4 fragmentColor;
// JOY_POST_DECLARATIONS_MACRO

/**
 * Passes the interpolated FORM preview color into the configurable post-processing chain.
 * Authored alpha remains untouched so the viewport blend state owns final compositing.
 */
void main() {
  // Preserve authored alpha for the viewport's configured blend state.
  vec4 sourceColor = vertexColor;

  // JOY_POST_APPLY_MACRO
}
