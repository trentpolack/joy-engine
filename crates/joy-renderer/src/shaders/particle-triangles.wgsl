// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Particle entry points for the arcade triangle renderer, whose games project triangles with their own shader and
// uniform block. Composed after particle-shapes.wgsl. Group 0 is this pass's own view block (the game's camera and
// billboard axes plus its color convention); group 1 is the sprite texture, or one white texel for untextured batches.
// Output is linear scene color; the triangle renderer's post chain presents it.

struct ParticleView {
  viewProjection: mat4x4f,
  // XYZ the axes billboards span and the direction toward the viewer; W unused.
  right: vec4f,
  up: vec4f,
  backward: vec4f,
  // X 1 when particle colors are display (sRGB) values to decode, as the game's own shader decodes its vertex colors;
  // Y a multiplier on the linear RGB (such as a frame flash); ZW unused.
  color: vec4f,
};

@group(0) @binding(0) var<uniform> view: ParticleView;
@group(1) @binding(0) var particleTexture: texture_2d<f32>;
@group(1) @binding(1) var particleTextureSampler: sampler;

/**
 * Decode a display (sRGB) color channel triple to linear light, extending the curve past one for HDR values.
 */
fn decodeDisplayColor(encoded: vec3f) -> vec3f {
  let magnitude = abs(encoded);
  let curve = pow((magnitude + vec3f(0.055))/1.055, vec3f(2.4));
  let segment = magnitude/12.92;
  return sign(encoded)*select(curve, segment, magnitude <= vec3f(0.04045));
}

/**
 * Apply the game's color convention to one shape layer's RGB.
 */
fn particleSourceColor(rgb: vec3f) -> vec3f {
  let linear = select(rgb, decodeDisplayColor(rgb), view.color.x > 0.5);
  return linear*view.color.y;
}

/**
 * Expand one quad corner along the supplied billboard axes.
 */
@vertex fn vertexMain(@builtin(vertex_index) vertexIndex: u32, instance: ParticleInstance) -> ParticleVarying {
  return expandParticle(vertexIndex, instance, view.right.xyz, view.up.xyz, view.backward.xyz, view.viewProjection);
}

/**
 * The fragment's premultiplied shape color. Sampling and derivatives happen here, before any per-shape branch.
 */
fn triangleParticle(input: ParticleVarying) -> vec4f {
  let texel = textureSample(particleTexture, particleTextureSampler, input.uv);
  let pixel = max(fwidth(input.local.x), fwidth(input.local.y));
  return shadeParticle(input, texel, pixel);
}

/**
 * Straight and premultiplied particles, composited source-over with premultiplied blending.
 */
@fragment fn fragmentOver(input: ParticleVarying) -> @location(0) vec4f {
  return triangleParticle(input);
}

/**
 * Additive particles.
 */
@fragment fn fragmentAdditive(input: ParticleVarying) -> @location(0) vec4f {
  return vec4f(triangleParticle(input).rgb, 0.0);
}
