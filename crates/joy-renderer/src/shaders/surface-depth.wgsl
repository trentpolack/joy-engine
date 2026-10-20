// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Alpha-tested depth pre-pass for masked materials: fills scene depth with the coverage the scene pass will shade, so
// cut-out holes stay open for whatever lies behind them. Composed after scene-common.wgsl, scene-vertex.wgsl,
// surface-contract.wgsl, environment-lighting.wgsl, and one model file; reads the material group at group 3. Positions
// use the scene pass's exact invariant expression.

struct MaskedVarying {
  @builtin(position) @invariant position: vec4f,
  @location(0) worldPosition: vec3f,
  @location(1) color: vec4f,
  @location(2) uv: vec2f,
};

/**
 * Project a masked vertex exactly as the scene pass does and forward what the model's opacity reads.
 */
@vertex fn maskedVertexMain(vertex: MeshVertex, instance: Instance) -> MaskedVarying {
  let world = instanceModel(instance)*vec4f(vertex.position, 1.0);
  var output: MaskedVarying;
  output.position = frame.viewProjection*world;
  output.worldPosition = world.xyz;
  output.color = vertex.color*instance.tint;
  output.uv = vertex.uv;
  return output;
}

/**
 * Discard fragments the material's alpha test removes; everything else only writes depth.
 */
@fragment fn maskedFragmentMain(interpolated: MaskedVarying, @builtin(front_facing) frontFacing: bool) {
  var input: SurfaceInput;
  input.worldPosition = interpolated.worldPosition;
  input.positionDx = dpdx(interpolated.worldPosition);
  input.positionDy = dpdy(interpolated.worldPosition);
  input.uv = interpolated.uv;
  input.uvDx = dpdx(interpolated.uv);
  input.uvDy = dpdy(interpolated.uv);
  input.viewDirection = normalize(frame.eye.xyz - interpolated.worldPosition);
  input.color = interpolated.color;
  input.frontFacing = frontFacing;
  if(materialOpacity(input) < material.coverage.x) {
    discard;
  }
}
