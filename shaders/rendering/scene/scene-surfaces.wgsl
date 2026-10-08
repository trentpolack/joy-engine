// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

struct Uniforms {
  viewProjection: mat4x4f,
  view: mat4x4f,
};
@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexOutput {
  @builtin(position) position: vec4f,
  @location(0) worldPosition: vec3f,
  @location(1) roughness: f32,
};

@vertex
fn vertexMain(@location(0) position: vec3f, @location(1) roughness: f32) -> VertexOutput {
  var output: VertexOutput;
  // Keep the same expression and world coordinates as the opaque scene shader.
  output.position = uniforms.viewProjection*vec4f(position, 1.0);
  output.worldPosition = position;
  output.roughness = roughness;
  return output;
}

@fragment
fn fragmentMain(input: VertexOutput) -> @location(0) vec4f {
  // Derivatives match the flat-lit scene's geometric plane, including scaled meshes.
  let surface = cross(dpdx(input.worldPosition), dpdy(input.worldPosition));
  let worldNormal = surface/max(length(surface), 0.000001);
  let transformed = (uniforms.view*vec4f(worldNormal, 0.0)).xyz;
  var viewNormal = transformed/max(length(transformed), 0.000001);
  let viewPosition = (uniforms.view*vec4f(input.worldPosition, 1.0)).xyz;
  // Lighting is double-sided. Orient the visible side toward the eye while keeping
  // signed components: GTAO and reflection rays depend on the complete direction.
  if(dot(viewNormal, -viewPosition) < 0.0) {
    viewNormal = -viewNormal;
  }
  return vec4f(viewNormal*0.5 + 0.5, clamp(input.roughness, 0.0, 1.0));
}
