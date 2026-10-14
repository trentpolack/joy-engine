// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

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

#ifdef JOY_SHADOWS
struct ShadowReceiver {viewMatrix: mat4x4f};
@group(1) @binding(0) var<uniform> receiver: ShadowReceiver;
#endif

fn sceneLighting(worldPosition: vec3f, color: vec4f, normal: vec3f) -> vec4f {
  var irradiance = vec3f(uniforms.lighting.y) + uniforms.skylightColorIntensity.rgb*uniforms.skylightColorIntensity.a;
  var sunShadow = 1.0;
#ifdef JOY_SHADOWS
  let viewDepth = -(receiver.viewMatrix*vec4f(worldPosition, 1)).z;
  sunShadow = shadow_getDirectionalFactor(worldPosition, normal, viewDepth);
#endif
  let atmosphereDiffuse = abs(dot(normal, uniforms.atmosphereDirectionIntensity.xyz));
  irradiance+= uniforms.atmosphereColor.rgb*uniforms.atmosphereDirectionIntensity.a*atmosphereDiffuse*sunShadow;

  for(var index = 0u; index < u32(uniforms.lighting.x); index++) {
    let light = uniforms.lights[index];
    let offset = light.positionRange.xyz - worldPosition;
    let distance = length(offset);
    let direction = offset/max(distance, 0.000001);
    let reach = max(0.0, 1.0 - (distance/light.positionRange.w));
    let diffuse = abs(dot(normal, direction));

    var visibility = 1.0;
#ifdef JOY_SHADOWS
    visibility = shadow_getPointFactor(i32(light.color.a) - 1, worldPosition, normal);
#endif
    irradiance+= light.color.rgb*pow(reach, 2.0)*diffuse*visibility;
  }

  return(vec4f(color.rgb*irradiance, color.a));
}
