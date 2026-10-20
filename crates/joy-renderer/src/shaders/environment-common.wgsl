// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Shared by the image-lighting capture passes: the per-draw uniforms, the full-screen triangle, cube face directions,
// and GGX importance sampling. Every pass draws one cube face (or the BRDF table) at a time and takes texel positions
// from the fragment position, so faces come out upright on every backend.

const ENVIRONMENT_PI: f32 = 3.141592653589793;

struct EnvironmentPass {
  // X cube face (+X, -X, +Y, -Y, +Z, -Z), Y destination edge in pixels, Z source edge in pixels (the radiance cube's
  // sharpest level), W importance samples per texel.
  destination: vec4f,
  // X perceptual roughness, Y source mip count, Z 1 to decode an sRGB source, W equirectangular taps per axis.
  parameters: vec4f,
};

@group(0) @binding(0) var<uniform> environmentPass: EnvironmentPass;

@vertex fn vertexMain(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
  let corner = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
  return vec4f((corner*2.0) - 1.0, 0.0, 1.0);
}

/**
 * The world direction through a face texel, for top-left texture coordinates in [0, 1] (the cube sampling
 * convention).
 */
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

/**
 * An orthonormal basis whose third axis is `normal`.
 */
fn tangentBasis(normal: vec3f) -> mat3x3f {
  var up = vec3f(1.0, 0.0, 0.0);
  if(abs(normal.z) < 0.999) {
    up = vec3f(0.0, 0.0, 1.0);
  }
  let tangent = normalize(cross(up, normal));
  return mat3x3f(tangent, cross(normal, tangent), normal);
}

/**
 * Hammersley point `index` of `count`; the radical inverse uses shifts and masks because WebGL 2 lacks bit reversal.
 */
fn hammersley(index: u32, count: u32) -> vec2f {
  var bits = (index << 16u) | (index >> 16u);
  bits = ((bits & 0x55555555u) << 1u) | ((bits & 0xAAAAAAAAu) >> 1u);
  bits = ((bits & 0x33333333u) << 2u) | ((bits & 0xCCCCCCCCu) >> 2u);
  bits = ((bits & 0x0F0F0F0Fu) << 4u) | ((bits & 0xF0F0F0F0u) >> 4u);
  bits = ((bits & 0x00FF00FFu) << 8u) | ((bits & 0xFF00FF00u) >> 8u);
  return vec2f(f32(index)/f32(count), f32(bits)*2.3283064365386963e-10);
}

/**
 * A half vector around +Z distributed by the GGX normal distribution for roughness `alpha` (perceptual roughness
 * squared), from a point in the unit square.
 */
fn ggxHalfVector(sequence: vec2f, alpha: f32) -> vec3f {
  let azimuth = 2.0*ENVIRONMENT_PI*sequence.x;
  let cosine = sqrt((1.0 - sequence.y)/max(1.0 + (((alpha*alpha) - 1.0)*sequence.y), 0.000001));
  let sine = sqrt(max(1.0 - (cosine*cosine), 0.0));
  return vec3f(cos(azimuth)*sine, sin(azimuth)*sine, cosine);
}

/**
 * The GGX normal distribution for the cosine between the normal and half vector.
 */
fn ggxDistribution(normalDotHalf: f32, alpha: f32) -> f32 {
  let squared = alpha*alpha;
  let denominator = ((normalDotHalf*normalDotHalf)*(squared - 1.0)) + 1.0;
  return squared/max(ENVIRONMENT_PI*denominator*denominator, 0.000001);
}
