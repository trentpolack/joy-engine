// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Particle entry points for the scene renderer's transparency queue. Composed after scene-common.wgsl (group 0: the
// frame uniforms, whose view matrix gives the camera axes, and the fog volume; group 1, the shadow receiver, is bound
// but unused) and particle-shapes.wgsl. Group 2 is the sprite texture: the batch's image, or one white texel for
// untextured batches. Colors are linear HDR as authored, depth-tested against opaque geometry without depth writes.

@group(2) @binding(0) var particleTexture: texture_2d<f32>;
@group(2) @binding(1) var particleTextureSampler: sampler;

/**
 * Scene particles use their linear HDR colors as authored.
 */
fn particleSourceColor(rgb: vec3f) -> vec3f {
  return rgb;
}

/**
 * Expand one quad corner facing the frame's camera.
 */
@vertex fn vertexMain(@builtin(vertex_index) vertexIndex: u32, instance: ParticleInstance) -> ParticleVarying {
  // The rows of the view matrix's rotation are the camera's right, up, and backward axes in world space.
  let right = vec3f(frame.view[0][0], frame.view[1][0], frame.view[2][0]);
  let up = vec3f(frame.view[0][1], frame.view[1][1], frame.view[2][1]);
  let backward = vec3f(frame.view[0][2], frame.view[1][2], frame.view[2][2]);
  return expandParticle(vertexIndex, instance, right, up, backward, frame.viewProjection);
}

/**
 * The fragment's premultiplied shape color. Sampling and derivatives happen here, before any per-shape branch.
 */
fn sceneParticle(input: ParticleVarying) -> vec4f {
  let texel = textureSample(particleTexture, particleTextureSampler, input.uv);
  let pixel = max(fwidth(input.local.x), fwidth(input.local.y));
  return shadeParticle(input, texel, pixel);
}

/**
 * Straight and premultiplied particles, composited source-over (premultiplied blending) and fogged like any surface.
 */
@fragment fn fragmentOver(input: ParticleVarying) -> @location(0) vec4f {
  let color = sceneParticle(input);
  let fog = volumetricFog(input.worldPosition);
  return vec4f((color.rgb*fog.a) + (fog.rgb*color.a), color.a);
}

/**
 * Additive particles: emitted light dims through fog but adds no in-scattering, since it hides nothing behind it.
 */
@fragment fn fragmentAdditive(input: ParticleVarying) -> @location(0) vec4f {
  let color = sceneParticle(input);
  let fog = volumetricFog(input.worldPosition);
  return vec4f(color.rgb*fog.a, 0.0);
}
