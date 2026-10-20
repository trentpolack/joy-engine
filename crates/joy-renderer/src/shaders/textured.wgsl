// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Textured transparent material for camera-facing particles and other straight-alpha sprites. Texture RGB is stored as
// sRGB and decoded to linear by the sampler, then multiplied by the linear HDR vertex color; alpha stays straight for
// source-over blending. Vertex colors carry any tint; there is no tint uniform. Prepended at load time by
// scene-common.wgsl, whose frame uniforms (group 0) provide the camera; group 1 (the shadow receiver) is bound but
// unused.

@group(2) @binding(0) var particleTexture: texture_2d<f32>;
@group(2) @binding(1) var particleTextureSampler: sampler;

struct TexturedVertex {
  @location(0) position: vec3f,
  @location(1) color: vec4f,
  @location(2) uv: vec2f,
};

struct TexturedOutput {
  @builtin(position) position: vec4f,
  @location(0) color: vec4f,
  @location(1) uv: vec2f,
  @location(2) worldPosition: vec3f,
};

/**
 * Project a world-space sprite vertex and forward its color and top-left-origin texture coordinates.
 */
@vertex fn vertexMain(vertex: TexturedVertex) -> TexturedOutput {
  var output: TexturedOutput;
  output.position = frame.viewProjection*vec4f(vertex.position, 1.0);
  output.color = vertex.color;
  output.uv = vertex.uv;
  output.worldPosition = vertex.position;
  return output;
}

/**
 * Sample the sprite texture and modulate it with the interpolated particle color, keeping straight alpha, then fog it.
 */
@fragment fn fragmentMain(input: TexturedOutput) -> @location(0) vec4f {
  let color = textureSample(particleTexture, particleTextureSampler, input.uv)*input.color;
  return applyFog(color, input.worldPosition);
}
