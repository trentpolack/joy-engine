// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Depth pre-pass for opaque meshes: fills scene depth front to back before any lighting runs, so the scene pass shades
// each visible pixel once. Positions use the scene pass's exact invariant expression. Composed after scene-common.wgsl
// and scene-vertex.wgsl; masked materials use surface-depth.wgsl with their shading model instead.

/**
 * Project an instanced mesh vertex exactly as the scene pass does.
 */
@vertex fn vertexMain(vertex: MeshVertex, instance: Instance) -> @builtin(position) @invariant vec4f {
  let world = instanceModel(instance)*vec4f(vertex.position, 1.0);
  return frame.viewProjection*world;
}
