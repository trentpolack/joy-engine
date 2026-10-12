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
 * Vertex shader for the shadow caster.
 */
@vertex fn vertexMain(@location(0) position: vec3f) -> @builtin(position) vec4f {
  return(casterCamera.viewProjection*vec4f(position, 1.0));
}

/**
 * Luma pipelines require a fragment stage even for depth-only attachments; it just, you know, does nothing.
 */
@fragment fn fragmentMain() {
  // no-op.
}
