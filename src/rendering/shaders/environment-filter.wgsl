// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
//
// Portions ported from luma.gl 9.4.2 `pbrEnvironmentFilter` (MIT License, Copyright (c) vis.gl contributors); see
// src/rendering/scene_effects/LICENSE-luma.gl.

// Image-based lighting integration: one draw per destination face and mip. Mode 0 prefilters specular radiance with
// GGX importance sampling at the draw's roughness, mode 1 integrates cosine-weighted diffuse irradiance, and mode 2
// integrates the split-sum BRDF table. Face directions follow the cube sampling convention with a top-left texel
// origin, taken from the fragment position so every backend writes upright faces.

const ENVIRONMENT_PI: f32 = 3.141592653589793;

struct Filter {
  face: u32,
  mode: u32,
  sampleCount: u32,
  // 1 decodes an sRGB-encoded source.
  sourceEncoding: u32,
  roughness: f32,
  size: f32,
  padding: vec2f,
};

@group(0) @binding(0) var<uniform> integration: Filter;
@group(0) @binding(1) var environmentSource: texture_2d<f32>;
@group(0) @binding(2) var environmentSampler: sampler;

@vertex fn vertexMain(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
  let corner = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
  return vec4f((corner*2.0) - 1.0, 0.0, 1.0);
}

/**
 * Hammersley point; the radical inverse is computed with shifts and masks because WebGL 2 lacks bit reversal.
 */
fn hammersley(index: u32, count: u32) -> vec2f {
  var bits = (index << 16u) | (index >> 16u);
  bits = ((bits & 0x55555555u) << 1u) | ((bits & 0xAAAAAAAAu) >> 1u);
  bits = ((bits & 0x33333333u) << 2u) | ((bits & 0xCCCCCCCCu) >> 2u);
  bits = ((bits & 0x0F0F0F0Fu) << 4u) | ((bits & 0xF0F0F0F0u) >> 4u);
  bits = ((bits & 0x00FF00FFu) << 8u) | ((bits & 0xFF00FF00u) >> 8u);
  return vec2f(f32(index)/f32(count), f32(bits)*2.3283064365386963e-10);
}

fn faceDirection(face: u32, textureCoordinate: vec2f) -> vec3f {
  let coordinate = (textureCoordinate*2.0) - 1.0;
  switch face {
    case 0u: { return normalize(vec3f(1.0, -coordinate.y, -coordinate.x)); }
    case 1u: { return normalize(vec3f(-1.0, -coordinate.y, coordinate.x)); }
    case 2u: { return normalize(vec3f(coordinate.x, 1.0, coordinate.y)); }
    case 3u: { return normalize(vec3f(coordinate.x, -1.0, -coordinate.y)); }
    case 4u: { return normalize(vec3f(coordinate.x, -coordinate.y, 1.0)); }
    default: { return normalize(vec3f(-coordinate.x, -coordinate.y, -1.0)); }
  }
}

fn tangentBasis(normal: vec3f) -> mat3x3f {
  var up = vec3f(1.0, 0.0, 0.0);
  if(abs(normal.z) < 0.999) {
    up = vec3f(0.0, 0.0, 1.0);
  }
  let tangent = normalize(cross(up, normal));
  return mat3x3f(tangent, cross(normal, tangent), normal);
}

/**
 * Equirectangular radiance in a direction: U wraps around Y from +X, V runs from straight up to straight down.
 */
fn sampleSource(direction: vec3f) -> vec3f {
  let unit = normalize(direction);
  let coordinate = vec2f((atan2(unit.z, unit.x)/(2.0*ENVIRONMENT_PI)) + 0.5, acos(clamp(unit.y, -1.0, 1.0))/ENVIRONMENT_PI);
  var color = textureSampleLevel(environmentSource, environmentSampler, coordinate, 0.0).rgb;
  if(integration.sourceEncoding == 1u) {
    color = select(color/12.92, pow((color + 0.055)/1.055, vec3f(2.4)), color >= vec3f(0.04045));
  }
  return color;
}

fn importanceSample(sequence: vec2f, normal: vec3f, roughness: f32) -> vec3f {
  let alpha = max(roughness*roughness, 0.0001);
  let azimuth = 2.0*ENVIRONMENT_PI*sequence.x;
  let cosine = sqrt((1.0 - sequence.y)/max(1.0 + (((alpha*alpha) - 1.0)*sequence.y), 0.0001));
  let sine = sqrt(max(1.0 - (cosine*cosine), 0.0));
  return normalize(tangentBasis(normal)*vec3f(cos(azimuth)*sine, sin(azimuth)*sine, cosine));
}

fn geometry(normalDotDirection: f32, roughness: f32) -> f32 {
  let factor = roughness*roughness*0.5;
  return normalDotDirection/max((normalDotDirection*(1.0 - factor)) + factor, 0.0001);
}

fn integrateBrdf(normalDotView: f32, roughness: f32, count: u32) -> vec2f {
  let view = vec3f(sqrt(max(1.0 - (normalDotView*normalDotView), 0.0)), 0.0, normalDotView);
  var integration = vec2f(0.0);
  for(var index = 0u; index < count; index++) {
    let halfVector = importanceSample(hammersley(index, count), vec3f(0.0, 0.0, 1.0), roughness);
    let light = normalize((2.0*dot(view, halfVector)*halfVector) - view);
    let normalDotLight = max(light.z, 0.0);
    let normalDotHalf = max(halfVector.z, 0.0);
    let viewDotHalf = max(dot(view, halfVector), 0.0);
    if(normalDotLight > 0.0) {
      let visibility = geometry(normalDotView, roughness)*geometry(normalDotLight, roughness)*viewDotHalf/
        max(normalDotHalf*normalDotView, 0.0001);
      let fresnel = pow(1.0 - viewDotHalf, 5.0);
      integration+= vec2f(1.0 - fresnel, fresnel)*visibility;
    }
  }
  return integration/f32(count);
}

fn integrateRadiance(normal: vec3f, count: u32) -> vec3f {
  var color = vec3f(0.0);
  var weight = 0.0;
  for(var index = 0u; index < count; index++) {
    let sequence = hammersley(index, count);
    var direction: vec3f;
    var sampleWeight: f32;
    if(integration.mode == 1u) {
      let azimuth = 2.0*ENVIRONMENT_PI*sequence.x;
      let cosine = sqrt(1.0 - sequence.y);
      let sine = sqrt(sequence.y);
      direction = normalize(tangentBasis(normal)*vec3f(cos(azimuth)*sine, sin(azimuth)*sine, cosine));
      sampleWeight = 1.0;
    } else {
      let halfVector = importanceSample(sequence, normal, integration.roughness);
      direction = normalize((2.0*dot(normal, halfVector)*halfVector) - normal);
      sampleWeight = max(dot(normal, direction), 0.0);
    }
    color+= sampleSource(direction)*sampleWeight;
    weight+= sampleWeight;
  }
  return color/max(weight, 0.0001);
}

@fragment fn fragmentMain(@builtin(position) position: vec4f) -> @location(0) vec4f {
  let textureCoordinate = position.xy/integration.size;
  let count = max(integration.sampleCount, 1u);
  if(integration.mode == 2u) {
    // Rows run from smooth (top) to rough (bottom), matching lookups at (n.v, roughness).
    return vec4f(integrateBrdf(max(textureCoordinate.x, 0.001), textureCoordinate.y, count), 0.0, 1.0);
  }
  return vec4f(integrateRadiance(faceDirection(integration.face, textureCoordinate), count), 1.0);
}
