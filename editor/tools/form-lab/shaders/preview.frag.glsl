precision highp float;

layout(std140) uniform uniforms {
  mat4 viewProjection;
  vec4 tint;
  // JOY_POST_UNIFORMS_MACRO
};

in vec4 vertexColor;
out vec4 fragmentColor;
// JOY_POST_DECLARATIONS_MACRO

void main() {
  // Preserve authored alpha for the viewport's configured blend state.
  vec4 sourceColor = vertexColor;
  // JOY_POST_APPLY_MACRO
}
