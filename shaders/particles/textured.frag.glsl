// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Camera-facing particle material.
 * Texture RGB is sampled as sRGB by the GPU and multiplied by linear HDR vertex tint; alpha remains straight.
 */

precision highp float;

layout(std140) uniform uniforms {
  mat4 viewProjection;
  vec4 tint;
};

uniform sampler2D particleTexture;

in vec2 textureUv;
in vec4 vertexColor;
out vec4 fragmentColor;

/**
 * Samples the particle texture and modulates it with interpolated particle color.
 * The result retains straight alpha for the blend state configured by the particle renderer.
 */
void main() {
  vec4 sourceColor = vertexColor*texture(particleTexture, textureUv);
  fragmentColor = sourceColor;
}
