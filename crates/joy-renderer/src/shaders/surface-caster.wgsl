// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Alpha-tested shadow caster for masked materials: texels below the cutoff leave holes in the depth map, so masked
// surfaces cast the coverage the camera sees. Composed as this file, scene-vertex.wgsl, surface-contract.wgsl,
// environment-lighting.wgsl, and one model file; group 0 is the caster's light view, group 3 the material. The scene's
// frame and shadow groups are not part of this module, which is why models never read them.

struct Caster {
  lightViewProjection: mat4x4f,
};

@group(0) @binding(0) var<uniform> caster: Caster;

struct CasterVarying {
  @builtin(position) position: vec4f,
  @location(0) worldPosition: vec3f,
  @location(1) color: vec4f,
  @location(2) uv: vec2f,
};

/**
 * Project a masked vertex into the light's view and forward what the model's opacity reads.
 */
@vertex fn vertexMain(vertex: MeshVertex, instance: Instance) -> CasterVarying {
  let world = instanceModel(instance)*vec4f(vertex.position, 1.0);
  var output: CasterVarying;
  output.position = caster.lightViewProjection*world;
  output.worldPosition = world.xyz;
  output.color = vertex.color*instance.tint;
  output.uv = vertex.uv;
  return output;
}

/**
 * Discard texels the material's alpha test removes.
 */
@fragment fn fragmentMain(interpolated: CasterVarying, @builtin(front_facing) frontFacing: bool) {
  var input: SurfaceInput;
  input.worldPosition = interpolated.worldPosition;
  input.positionDx = dpdx(interpolated.worldPosition);
  input.positionDy = dpdy(interpolated.worldPosition);
  input.uv = interpolated.uv;
  input.uvDx = dpdx(interpolated.uv);
  input.uvDy = dpdy(interpolated.uv);
  input.color = interpolated.color;
  input.frontFacing = frontFacing;
  if(materialOpacity(input) < material.coverage.x) {
    discard;
  }
}
