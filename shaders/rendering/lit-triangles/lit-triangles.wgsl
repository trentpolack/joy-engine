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

struct VertexOutput {
  @builtin(position) position: vec4f,
  @location(0) color: vec4f,
  @location(1) worldPosition: vec3f,
};

@vertex
fn vertexMain(@location(0) position: vec3f, @location(1) color: vec4f) -> VertexOutput {
  var output: VertexOutput;
  output.position = uniforms.viewProjection*vec4f(position, 1.0);
  output.worldPosition = position;
  output.color = color*uniforms.tint;

  return output;
}

@fragment
fn fragmentMain(input: VertexOutput) -> @location(0) vec4f {
  let surface = cross(dpdx(input.worldPosition), dpdy(input.worldPosition));
  let normal = surface / max(length(surface), 0.000001);
  var irradiance = vec3f(uniforms.lighting.y) + uniforms.skylightColorIntensity.rgb*uniforms.skylightColorIntensity.a;
  let atmosphereDiffuse = abs(dot(normal, uniforms.atmosphereDirectionIntensity.xyz));
  irradiance+= uniforms.atmosphereColor.rgb*uniforms.atmosphereDirectionIntensity.a*atmosphereDiffuse;
  for(var index = 0u; index < u32(uniforms.lighting.x); index++) {
    let light = uniforms.lights[index];
    let offset = light.positionRange.xyz - input.worldPosition;
    let distance = length(offset);
    let direction = offset / max(distance, 0.000001);
    let reach = max(0.0, 1.0 - distance / light.positionRange.w);
    let diffuse = abs(dot(normal, direction));

    irradiance+= light.color.rgb*pow(reach, 2.0)*diffuse;
  }

  return(vec4f(input.color.rgb*irradiance, input.color.a));
}
