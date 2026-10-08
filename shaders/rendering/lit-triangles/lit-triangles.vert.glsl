// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Matches the two-vec4 light records packed by createPointLightUniforms.
struct PointLight {
  vec4 positionRange;
  vec4 color;
};

layout(std140) uniform uniforms {
  mat4 viewProjection;
  vec4 tint;
  vec4 lighting;
  PointLight lights[32];
  vec4 atmosphereDirectionIntensity;
  vec4 atmosphereColor;
  vec4 skylightColorIntensity;
};

layout(location = 0) in vec3 position;
layout(location = 1) in vec4 color;
out vec4 vertexColor;
out vec3 worldPosition;

void main() {
  gl_Position = viewProjection*vec4(position, 1.0);
  worldPosition = position;
  vertexColor = color*tint;
}
