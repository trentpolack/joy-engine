// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Lit material: flat, double-sided vertex-color lighting. This is the engine's standard look for primitives and FORM
// meshes; normals come from screen-space derivatives so faceted geometry reads the same as the TypeScript scene path.
// Prepended at load time by scene-common.wgsl.

struct LitOutput {
  // Invariant so the depth pre-pass and every scene pipeline rasterize identical depth.
  @builtin(position) @invariant position: vec4f,
  @location(0) worldPosition: vec3f,
  @location(1) color: vec4f,
  @location(2) receivesShadow: f32,
  @location(3) sceneRoughness: f32,
  @location(4) currentClip: vec4f,
  @location(5) previousClip: vec4f,
};

/**
 * Transform an instanced mesh vertex to clip space and apply the instance tint and opacity.
 */
@vertex fn vertexMain(vertex: MeshVertex, instance: Instance) -> LitOutput {
  let world = instanceModel(instance)*vec4f(vertex.position, 1.0);
  let clips = motionClips(world, instancePreviousModel(instance)*vec4f(vertex.position, 1.0));

  var output: LitOutput;
  output.position = frame.viewProjection*world;
  output.worldPosition = world.xyz;
  output.color = vertex.color*instance.tint;
  output.receivesShadow = instance.flags.x;
  output.sceneRoughness = instance.flags.y;
  output.currentClip = clips[0];
  output.previousClip = clips[1];
  return output;
}

/**
 * Light the interpolated vertex color with a derivative normal oriented toward the eye. `surface` is the derivative
 * cross product, taken by the entry point in uniform control flow.
 */
fn litColor(input: LitOutput, surface: vec3f) -> vec4f {
  let normal = faceEye(surface/max(length(surface), 0.000001), input.worldPosition);
  let lit = standardLighting(input.worldPosition, input.color.rgb, normal, input.receivesShadow);
  return vec4f(lit, input.color.a);
}

/**
 * Color only.
 */
@fragment fn fragmentMain(input: LitOutput) -> @location(0) vec4f {
  let surface = cross(dpdx(input.worldPosition), dpdy(input.worldPosition));
  return litColor(input, surface);
}

/**
 * Color plus the scene-effect attachments.
 */
@fragment fn fragmentTargets(input: LitOutput) -> SceneTargets {
  let surface = cross(dpdx(input.worldPosition), dpdy(input.worldPosition));
  let clips = mat2x4f(input.currentClip, input.previousClip);
  return sceneTargets(litColor(input, surface), input.worldPosition, surface, input.sceneRoughness, clips);
}
