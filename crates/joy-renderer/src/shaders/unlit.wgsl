// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Unlit material: emits tinted vertex color unchanged. Transparent instances and transient effect triangles use it,
// matching the TypeScript scene path where transparency is unlit. Prepended at load time by scene-common.wgsl.

struct UnlitOutput {
  // Invariant so the depth pre-pass and every scene pipeline rasterize identical depth.
  @builtin(position) @invariant position: vec4f,
  @location(0) color: vec4f,
  @location(1) worldPosition: vec3f,
  @location(2) sceneRoughness: f32,
  @location(3) currentClip: vec4f,
  @location(4) previousClip: vec4f,
};

/**
 * Transform an instanced mesh vertex to clip space and apply the instance tint and opacity.
 */
@vertex fn vertexMain(vertex: MeshVertex, instance: Instance) -> UnlitOutput {
  let world = instanceModel(instance)*vec4f(vertex.position, 1.0);
  let clips = motionClips(world, instancePreviousModel(instance)*vec4f(vertex.position, 1.0));

  var output: UnlitOutput;
  output.position = frame.viewProjection*world;
  output.color = vertex.color*instance.tint;
  output.worldPosition = world.xyz;
  output.sceneRoughness = instance.flags.y;
  output.currentClip = clips[0];
  output.previousClip = clips[1];
  return output;
}

/**
 * Write straight-alpha linear color for source-over blending into the HDR scene.
 */
@fragment fn fragmentMain(input: UnlitOutput) -> @location(0) vec4f {
  return input.color;
}

/**
 * Opaque color plus the scene-effect attachments.
 */
@fragment fn fragmentTargets(input: UnlitOutput) -> SceneTargets {
  let surface = cross(dpdx(input.worldPosition), dpdy(input.worldPosition));
  let clips = mat2x4f(input.currentClip, input.previousClip);
  return sceneTargets(input.color, input.worldPosition, surface, input.sceneRoughness, clips);
}
