// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Flat surface normals keep the existing XYZ/RGBA vertex format portable.
struct VertexOutput {
  @builtin(position) position: vec4f,
  @location(0) color: vec4f,
  @location(1) worldPosition: vec3f,
};

/**
 * Projects a lit shadow receiver and forwards its world-space lighting inputs.
 * The light-space position is emitted alongside the camera-space position for shadow comparison in the fragment stage.
 */
@vertex
fn vertexMain(
  @location(0) position: vec3f,
  @location(1) color: vec4f
  ) -> VertexOutput {
  var output: VertexOutput;
  output.position = uniforms.viewProjection*vec4f(position, 1.0);
  output.worldPosition = position;
  output.color = color*uniforms.tint;

  return output;
}

/**
 * Lights a receiver while attenuating direct light with the shadow map.
 * Depth bias and filtered comparison reduce self-shadowing without affecting ambient illumination.
 */
@fragment
fn fragmentMain(input: VertexOutput) -> @location(0) vec4f {
  // WebGPU framebuffer Y points down (top-left corner is the origin). Reverse the derivative cross so receiver normal bias moves toward the visible surface rather than into its caster.
  let surface = cross(dpdy(input.worldPosition), dpdx(input.worldPosition));
  let normal = surface / max(length(surface), 0.000001);
  return sceneLighting(input.worldPosition, input.color, normal);
}
