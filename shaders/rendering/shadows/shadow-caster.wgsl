// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Uniforms for the caster camera.
 */
struct CasterCamera {
  viewProjection: mat4x4f
};

@group(0) @binding(0) var<uniform> casterCamera: CasterCamera;

/**
 * Projects untextured geometry into the shadow caster camera.
 * The depth-only pipeline needs no interpolants beyond the built-in clip-space position.
 */
@vertex fn vertexMain(@location(0) position: vec3f) -> @builtin(position) vec4f {
  return(casterCamera.viewProjection*vec4f(position, 1.0));
}

/**
 * Completes the depth-only shadow pipeline without writing a color target.
 *
 * Luma requires a fragment stage even though rasterized depth is the only retained result.
 */
@fragment fn fragmentMain() {
  // no-op.
}
