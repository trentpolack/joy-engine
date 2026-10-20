// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Image-based lighting for shading models: the prepared environment group (group 2) and the helpers models call from
// `materialIndirect`. Without an environment map the group binds zero irradiance and a black specular cube and
// `AmbientLight.environment.x` is zero, so models skip it. The environment is rotated about the world Y axis by
// `AmbientLight.environment.y` radians.

struct ImageLighting {
  // X intensity (zero disables image lighting), Y horizontal rotation in radians, Z highest specular mip.
  parameters: vec4f,
};

struct EnvironmentIrradiance {
  // L2 spherical-harmonic irradiance over pi (RGB) for the polynomials 1, y, z, x, xy, yz, 3z² - 1, xz, and x² - y²
  // of the environment-space normal, with the basis constants folded in (irradiance.rs).
  coefficients: array<vec4f, 9>,
};

@group(2) @binding(0) var<uniform> imageLighting: ImageLighting;
@group(2) @binding(1) var<uniform> environmentIrradiance: EnvironmentIrradiance;
@group(2) @binding(2) var specularEnvironment: texture_cube<f32>;
@group(2) @binding(3) var brdfTable: texture_2d<f32>;
@group(2) @binding(4) var environmentSampler: sampler;

/**
 * A world direction in the environment map's frame.
 */
fn environmentDirection(ambient: AmbientLight, direction: vec3f) -> vec3f {
  let cosine = cos(ambient.environment.y);
  let sine = sin(ambient.environment.y);
  return vec3f((cosine*direction.x) - (sine*direction.z), direction.y, (sine*direction.x) + (cosine*direction.z));
}

/**
 * Cosine-weighted average radiance arriving around a unit `normal` (irradiance over pi), intensity applied.
 */
fn environmentDiffuse(ambient: AmbientLight, normal: vec3f) -> vec3f {
  let n = environmentDirection(ambient, normal);
  let c = environmentIrradiance.coefficients;
  let band01 = c[0].rgb + (c[1].rgb*n.y) + (c[2].rgb*n.z) + (c[3].rgb*n.x);
  let band2 = (c[4].rgb*(n.x*n.y)) + (c[5].rgb*(n.y*n.z)) + (c[6].rgb*((3.0*n.z*n.z) - 1.0)) + (c[7].rgb*(n.x*n.z))
    + (c[8].rgb*((n.x*n.x) - (n.y*n.y)));
  return max(band01 + band2, vec3f(0.0))*ambient.environment.x;
}

/**
 * Prefiltered radiance around the reflection `direction` for a lobe of perceptual `roughness`, intensity applied. Mips
 * hold roughness r at level r(2 - r) of the highest (capture.rs prefilters each level at the inverse), which spends
 * more levels on glossy lobes, where the eye notices blur.
 */
fn environmentSpecular(ambient: AmbientLight, direction: vec3f, roughness: f32) -> vec3f {
  let r = clamp(roughness, 0.0, 1.0);
  let level = r*(2.0 - r)*ambient.environment.z;
  let sampleDirection = environmentDirection(ambient, direction);
  return textureSampleLevel(specularEnvironment, environmentSampler, sampleDirection, level).rgb*ambient.environment.x;
}

/**
 * The split-sum scale (X) and bias (Y) applied to F0 and F90 for a view angle `nv` and perceptual `roughness`.
 */
fn environmentBrdf(nv: f32, roughness: f32) -> vec2f {
  return textureSampleLevel(brdfTable, environmentSampler, vec2f(nv, roughness), 0.0).rg;
}
