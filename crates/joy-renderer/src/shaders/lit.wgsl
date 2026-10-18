// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Lit material: flat, double-sided vertex-color lighting. This is the engine's standard look for primitives and FORM
// meshes; normals come from screen-space derivatives so faceted geometry reads the same as the TypeScript scene path.
// Prepended at load time by scene-common.wgsl.

struct LitOutput {
  @builtin(position) position: vec4f,
  @location(0) worldPosition: vec3f,
  @location(1) color: vec4f,
  @location(2) receivesShadow: f32,
};

/**
 * Transform an instanced mesh vertex to clip space and apply the instance tint and opacity.
 */
@vertex fn vertexMain(vertex: MeshVertex, instance: Instance) -> LitOutput {
  let world = instanceModel(instance)*vec4f(vertex.position, 1.0);

  var output: LitOutput;
  output.position = frame.viewProjection*world;
  output.worldPosition = world.xyz;
  output.color = vertex.color*instance.tint;
  output.receivesShadow = instance.flags.x;
  return output;
}

/**
 * Light the interpolated vertex color with a derivative normal oriented toward the eye.
 * Derivatives are taken before any branch so they stay in uniform control flow.
 */
@fragment fn fragmentMain(input: LitOutput) -> @location(0) vec4f {
  let surface = cross(dpdx(input.worldPosition), dpdy(input.worldPosition));
  let normal = faceEye(surface/max(length(surface), 0.000001), input.worldPosition);
  let lit = standardLighting(input.worldPosition, input.color.rgb, normal, input.receivesShadow);
  return vec4f(lit, input.color.a);
}
