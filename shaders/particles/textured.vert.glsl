/**
 * Camera-facing particle material.
 * Texture RGB is sampled as sRGB by the GPU and multiplied by linear HDR vertex tint; alpha remains straight.
 */

layout(std140) uniform uniforms {
  mat4 viewProjection;
  vec4 tint;
};

layout(location = 0) in vec3 position;
layout(location = 1) in vec4 color;
layout(location = 2) in vec2 uv;

out vec2 textureUv;
out vec4 vertexColor;

void main() {
  textureUv = uv;
  gl_Position = viewProjection * vec4(position, 1.0);
  vertexColor = color*tint;
}
