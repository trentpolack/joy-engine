// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

struct TerrainSurfaceUniforms {
  viewProjection: mat4x4f,
  view: mat4x4f,
  roughness: vec4f,
};
@group(0) @binding(0) var<uniform> attachment: TerrainSurfaceUniforms;
@group(3) @binding(4) var<uniform> terrainModel: mat4x4f;

@vertex fn vertexMain(input: VertexInput) -> VertexOutput {
  var output = terrainSurface(input);
  output.worldPosition = (terrainModel*vec4f(output.worldPosition, 1.0)).xyz;
  output.position = attachment.viewProjection*vec4f(output.worldPosition, 1.0);
  return output;
}

@fragment fn fragmentMain(input: VertexOutput) -> @location(0) vec4f {
  let surface = cross(dpdx(input.worldPosition), dpdy(input.worldPosition));
  let worldNormal = surface/max(length(surface), 0.000001);
  var viewNormal = (attachment.view*vec4f(worldNormal, 0.0)).xyz;
  viewNormal/= max(length(viewNormal), 0.000001);
  let viewPosition = (attachment.view*vec4f(input.worldPosition, 1.0)).xyz;
  if(dot(viewNormal, -viewPosition) < 0.0) {
    viewNormal = -viewNormal;
  }
  return vec4f((viewNormal*0.5) + 0.5, attachment.roughness.x);
}
