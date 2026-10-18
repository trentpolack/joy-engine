// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Scene effect attachment: view-space normals and roughness for opaque geometry, drawn against the scene's depth with
// an equal depth test. Follows scene-common.wgsl, whose frame uniforms and vertex transform this must match exactly.

struct SurfaceOutput {
  @builtin(position) position: vec4f,
  @location(0) worldPosition: vec3f,
  @location(1) roughness: f32,
};

/**
 * Project geometry with the scene shaders' exact expression so depth matches, forwarding the instance's scene roughness.
 */
@vertex fn vertexMain(vertex: MeshVertex, instance: Instance) -> SurfaceOutput {
  let world = instanceModel(instance)*vec4f(vertex.position, 1.0);
  var output: SurfaceOutput;
  output.position = frame.viewProjection*world;
  output.worldPosition = world.xyz;
  output.roughness = instance.flags.y;
  return output;
}

/**
 * Encode the derivative normal in view space, oriented toward the eye, with roughness in alpha.
 */
@fragment fn fragmentMain(input: SurfaceOutput) -> @location(0) vec4f {
  // Derivatives match the flat-lit scene's geometric plane, including scaled meshes.
  let surface = cross(dpdx(input.worldPosition), dpdy(input.worldPosition));
  let worldNormal = surface/max(length(surface), 0.000001);
  let transformed = (frame.view*vec4f(worldNormal, 0.0)).xyz;
  var viewNormal = transformed/max(length(transformed), 0.000001);
  let viewPosition = (frame.view*vec4f(input.worldPosition, 1.0)).xyz;

  // Lighting is double-sided. Orient the visible side toward the eye while keeping signed components: GTAO and
  // reflection rays depend on the complete direction.
  if(dot(viewNormal, -viewPosition) < 0.0) {
    viewNormal = -viewNormal;
  }
  return vec4f((viewNormal*0.5) + 0.5, clamp(input.roughness, 0.0, 1.0));
}
