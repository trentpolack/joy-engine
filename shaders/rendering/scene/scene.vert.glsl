// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

layout(std140) uniform uniforms {
  mat4 viewProjection;
  vec4 tint;
  vec4 lighting;
  vec4 lights[64];
  vec4 atmosphereLighting[3];
  // JOY_POST_UNIFORMS_MACRO
};

layout(location = 0) in vec3 position;
layout(location = 1) in vec4 color;
out vec4 vertexColor;

void main() {
  gl_Position = viewProjection * vec4(position, 1.0);
  vertexColor = color*tint;
}
