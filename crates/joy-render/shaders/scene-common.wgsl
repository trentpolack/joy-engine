// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Shared scene contract for every surface material: frame uniforms, mesh and instance inputs, directional shadow
// sampling, and the engine's two lighting models. Each material shader is this file followed by its own entry points.

const PI: f32 = 3.14159265359;
const MAX_POINT_LIGHTS: u32 = 32u;

struct PointLight {
  // World position and the distance at which the light reaches zero.
  positionRange: vec4f,
  // Linear RGB irradiance.
  color: vec4f,
};

struct Frame {
  viewProjection: mat4x4f,
  view: mat4x4f,
  // XYZ eye position in world units; W is scene time in seconds.
  eye: vec4f,
  // X point light count, Y ambient irradiance, Z directional shadows enabled, W unused.
  lighting: vec4f,
  // XYZ direction toward the environment's primary light; W intensity.
  primaryDirection: vec4f,
  // RGB primary light color.
  primaryColor: vec4f,
  // RGB skylight color; A intensity.
  skylight: vec4f,
  shadowViewProjection: mat4x4f,
  // X receiver normal bias in world units, Y shadow-map texel size, Z filter radius in texels, W unused.
  shadowParameters: vec4f,
  lights: array<PointLight, 32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(0) var shadowMap: texture_depth_2d;
@group(1) @binding(1) var shadowSampler: sampler_comparison;

struct MeshVertex {
  @location(0) position: vec3f,
  @location(1) normal: vec3f,
  @location(2) color: vec4f,
  // X metallic, Y roughness, Z alpha cutoff, W alpha mode (0 opaque, 1 mask, 2 blend).
  @location(3) surface: vec4f,
  // RGB emission.
  @location(4) emissive: vec4f,
};

struct Instance {
  @location(5) model0: vec4f,
  @location(6) model1: vec4f,
  @location(7) model2: vec4f,
  @location(8) model3: vec4f,
  // RGB tint and opacity.
  @location(9) tint: vec4f,
  // X receives shadows (0 or 1); remaining components are reserved.
  @location(10) flags: vec4f,
};

/**
 * Rebuild the instance's column-major model matrix from its four vertex-attribute columns.
 */
fn instanceModel(instance: Instance) -> mat4x4f {
  return mat4x4f(instance.model0, instance.model1, instance.model2, instance.model3);
}

/**
 * Transform a model-space normal into world space with the model's cofactor matrix.
 * The cofactor is the inverse transpose scaled by the determinant, so normalizing it handles non-uniform scale and
 * negative scale without a matrix inverse.
 */
fn worldNormal(model: mat4x4f, normal: vec3f) -> vec3f {
  let column0 = model[0].xyz;
  let column1 = model[1].xyz;
  let column2 = model[2].xyz;
  let cofactor = mat3x3f(cross(column1, column2), cross(column2, column0), cross(column0, column1));
  let transformed = cofactor*normal;
  return transformed/max(length(transformed), 0.000001);
}

/**
 * Orient a world-space normal toward the eye, giving double-sided surfaces a consistent front for lighting and bias.
 */
fn faceEye(normal: vec3f, worldPosition: vec3f) -> vec3f {
  if(dot(normal, frame.eye.xyz - worldPosition) < 0.0) {
    return -normal;
  }
  return normal;
}

/**
 * Attenuate the primary light with a filtered directional shadow-map comparison.
 * The receiver is displaced along its normal before projection so flat ground does not shadow itself. Positions outside
 * the shadow volume are fully lit. The comparison uses an explicit level so it is valid outside uniform control flow.
 */
fn directionalShadow(worldPosition: vec3f, normal: vec3f) -> f32 {
  if(frame.lighting.z < 0.5) {
    return 1.0;
  }

  let biased = worldPosition + (normal*frame.shadowParameters.x);
  let clip = frame.shadowViewProjection*vec4f(biased, 1.0);
  let projected = clip.xyz/clip.w;
  let coordinate = vec2f((projected.x*0.5) + 0.5, 0.5 - (projected.y*0.5));
  if(any(coordinate < vec2f(0.0)) || any(coordinate > vec2f(1.0)) || (projected.z > 1.0) || (projected.z < 0.0)) {
    return 1.0;
  }

  // 3x3 percentage-closer filter scaled by the configured radius.
  let step = frame.shadowParameters.y*frame.shadowParameters.z;
  var visibility = 0.0;
  for(var y = -1; y <= 1; y++) {
    for(var x = -1; x <= 1; x++) {
      let offset = vec2f(f32(x), f32(y))*step;
      visibility+= textureSampleCompareLevel(shadowMap, shadowSampler, coordinate + offset, projected.z);
    }
  }

  return visibility/9.0;
}

/**
 * The engine's standard flat lighting: ambient plus colored skylight, a double-sided primary light, and point lights.
 * Matches the TypeScript scene path; diffuse terms use |n.l| without a 1/pi factor so existing scenes keep their
 * authored brightness.
 */
fn standardLighting(worldPosition: vec3f, color: vec3f, normal: vec3f, receivesShadow: f32) -> vec3f {
  var irradiance = vec3f(frame.lighting.y) + (frame.skylight.rgb*frame.skylight.a);

  var sunVisibility = 1.0;
  if(receivesShadow > 0.5) {
    sunVisibility = directionalShadow(worldPosition, normal);
  }
  let primaryDiffuse = abs(dot(normal, frame.primaryDirection.xyz));
  irradiance+= frame.primaryColor.rgb*frame.primaryDirection.w*primaryDiffuse*sunVisibility;

  let lightCount = min(u32(frame.lighting.x), MAX_POINT_LIGHTS);
  for(var index = 0u; index < lightCount; index++) {
    let light = frame.lights[index];
    let offset = light.positionRange.xyz - worldPosition;
    let distance = length(offset);
    let direction = offset/max(distance, 0.000001);
    let reach = max(0.0, 1.0 - (distance/light.positionRange.w));
    irradiance+= light.color.rgb*(reach*reach)*abs(dot(normal, direction));
  }

  return color*irradiance;
}

/**
 * Metallic-roughness response to one light: GGX specular with energy-conserving Lambertian diffuse.
 * Matches the TypeScript PBR material pass; the caller applies light color, attenuation, and visibility.
 */
fn pbrDirect(base: vec3f, metallic: f32, roughness: f32, n: vec3f, v: vec3f, l: vec3f) -> vec3f {
  let halfDirection = v + l;
  let h = halfDirection/max(length(halfDirection), 0.000001);
  let nv = max(dot(n, v), 0.0001);
  let nl = max(dot(n, l), 0.0);
  let nh = max(dot(n, h), 0.0);
  let vh = max(dot(v, h), 0.0);
  let a2 = pow(roughness, 4.0);
  let denominator = (nh*nh*(a2 - 1.0)) + 1.0;
  let distribution = a2/max(PI*denominator*denominator, 0.000000000001);
  let visibility = 0.5/max((nl*sqrt((nv*nv*(1.0 - a2)) + a2)) + (nv*sqrt((nl*nl*(1.0 - a2)) + a2)), 0.0001);
  let f0 = mix(vec3f(0.04), base, metallic);
  let fresnel = f0 + ((vec3f(1.0) - f0)*pow(1.0 - vh, 5.0));
  let diffuse = (vec3f(1.0) - fresnel)*(1.0 - metallic)*base/PI;

  return (diffuse + (distribution*visibility*fresnel))*nl;
}
