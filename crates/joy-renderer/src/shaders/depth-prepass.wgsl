// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Depth pre-pass for opaque meshes: fills scene depth front to back before any lighting runs, so the scene pass shades
// each visible pixel once. Positions use the scene materials' exact invariant expression. Masked submeshes alpha-test
// their base color like the shadow casters do. Prepended at load time by scene-common.wgsl; the masked entry points
// read the material group at group 1 (the scene's shadow group is never bound here).

@group(1) @binding(1) var baseColorMap: texture_2d<f32>;
@group(1) @binding(7) var materialSampler: sampler;

/**
 * Project an instanced mesh vertex exactly as the scene materials do.
 */
@vertex fn vertexMain(vertex: MeshVertex, instance: Instance) -> @builtin(position) @invariant vec4f {
  let world = instanceModel(instance)*vec4f(vertex.position, 1.0);
  return frame.viewProjection*world;
}

struct MaskedOutput {
  @builtin(position) @invariant position: vec4f,
  @location(0) uv: vec2f,
  @location(1) alpha: f32,
  // Alpha cutoff (X) and alpha mode (Y).
  @location(2) alphaTest: vec2f,
};

/**
 * Project a masked vertex and forward what the alpha test reads.
 */
@vertex fn maskedVertexMain(vertex: MeshVertex, instance: Instance, @location(15) uv: vec2f) -> MaskedOutput {
  let world = instanceModel(instance)*vec4f(vertex.position, 1.0);
  var output: MaskedOutput;
  output.position = frame.viewProjection*world;
  output.uv = uv;
  output.alpha = (vertex.color*instance.tint).a;
  output.alphaTest = vertex.surface.zw;
  return output;
}

/**
 * Discard fragments the material's alpha test removes; everything else only writes depth.
 */
@fragment fn maskedFragmentMain(input: MaskedOutput) {
  let alpha = textureSample(baseColorMap, materialSampler, input.uv).a*input.alpha;
  if((input.alphaTest.y > 0.5) && (input.alphaTest.y < 1.5) && (alpha < input.alphaTest.x)) {
    discard;
  }
}
