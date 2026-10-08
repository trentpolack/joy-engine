// Copyright (c) 2026 Trent Polack. Licensed under the MIT License.

// World-space PBR attributes. The caller owns transforms before submission.
layout(std140) uniform frameUniforms {
  mat4 viewProjection;
  vec4 eye;
  vec4 lightDirection;
  vec4 lightColor;
  vec4 ambientColor;
};

layout(location = 0) in vec3 position;
layout(location = 1) in vec3 normal;
layout(location = 2) in vec4 color;
layout(location = 3) in vec2 uv;

out vec3 worldPosition;
out vec3 worldNormal;
out vec4 vertexColor;
out vec2 texCoord;

void main() {
  gl_Position = viewProjection*vec4(position, 1.0);
  worldPosition = position;
  worldNormal = normal;
  vertexColor = color;
  texCoord = uv;
}
