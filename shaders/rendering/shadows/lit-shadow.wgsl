// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Flat surface normals keep the existing XYZ/RGBA vertex format portable.
struct PointLight {
  positionRange: vec4f,
  color: vec4f,
};

struct Uniforms {
  viewProjection: mat4x4f,
  tint: vec4f,
  lighting: vec4f,
  lights: array<PointLight, 32>,
  atmosphereDirectionIntensity: vec4f,
  atmosphereColor: vec4f,
  skylightColorIntensity: vec4f,
};
@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct ShadowReceiver {viewMatrix: mat4x4f};
@group(1) @binding(0) var<uniform> receiver: ShadowReceiver;

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
  var irradiance = vec3f(uniforms.lighting.y) + uniforms.skylightColorIntensity.rgb*uniforms.skylightColorIntensity.a;
  let viewDepth = -(receiver.viewMatrix*vec4f(input.worldPosition, 1)).z;
  let sunShadow = shadow_getDirectionalFactor(input.worldPosition, normal, viewDepth);
  let atmosphereDiffuse = abs(dot(normal, uniforms.atmosphereDirectionIntensity.xyz));
  irradiance+= uniforms.atmosphereColor.rgb*uniforms.atmosphereDirectionIntensity.a*atmosphereDiffuse*sunShadow;

  for(var index = 0u; index < u32(uniforms.lighting.x); index++) {
    let light = uniforms.lights[index];
    let offset = light.positionRange.xyz - input.worldPosition;
    let distance = length(offset);
    let direction = offset/max(distance, 0.000001);
    let reach = max(0.0, 1.0 - (distance/light.positionRange.w));
    let diffuse = abs(dot(normal, direction));

    let visibility = shadow_getPointFactor(i32(light.color.a) - 1, input.worldPosition, normal);
    irradiance+= light.color.rgb*pow(reach, 2.0)*diffuse*visibility;
  }

  return(vec4f(input.color.rgb*irradiance, input.color.a));
}
