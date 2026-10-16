// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Unlit material: emits tinted vertex color unchanged. Transparent instances and transient effect triangles use it,
// matching the TypeScript scene path where transparency is unlit. Prepended at load time by scene-common.wgsl.

struct UnlitOutput {
  @builtin(position) position: vec4f,
  @location(0) color: vec4f,
};

/**
 * Transform an instanced mesh vertex to clip space and apply the instance tint and opacity.
 */
@vertex fn vertexMain(vertex: MeshVertex, instance: Instance) -> UnlitOutput {
  let world = instanceModel(instance)*vec4f(vertex.position, 1.0);

  var output: UnlitOutput;
  output.position = frame.viewProjection*world;
  output.color = vertex.color*instance.tint;
  return output;
}

/**
 * Write straight-alpha linear color for source-over blending into the HDR scene.
 */
@fragment fn fragmentMain(input: UnlitOutput) -> @location(0) vec4f {
  return input.color;
}
