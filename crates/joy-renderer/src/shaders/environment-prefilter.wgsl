// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Specular prefiltering: each level of the specular cube holds the radiance cube convolved with the GGX lobe of one
// perceptual roughness, assuming the view and reflection directions equal the normal (the split-sum approximation).
// Samples are importance sampled from the lobe and filtered (Krivanek and Colbert 2008, "Real-time Shading with
// Filtered Importance Sampling"): each reads the radiance cube's mip whose texels cover the sample's share of the
// lobe's solid angle, so a few dozen samples integrate without noise or fireflies. Roughness zero copies the mirror
// image. Composed after environment-common.wgsl.

@group(0) @binding(2) var environmentSampler: sampler;
@group(0) @binding(3) var sourceCube: texture_cube<f32>;

@fragment fn prefilterMain(@builtin(position) position: vec4f) -> @location(0) vec4f {
  let face = u32(environmentPass.destination.x);
  let normal = faceDirection(face, position.xy/environmentPass.destination.y);
  let roughness = environmentPass.parameters.x;
  if(roughness <= 0.0) {
    return vec4f(textureSampleLevel(sourceCube, environmentSampler, normal, 0.0).rgb, 1.0);
  }

  let alpha = max(roughness*roughness, 0.0001);
  let count = max(u32(environmentPass.destination.w), 1u);
  let sourceSize = environmentPass.destination.z;
  let texelSolidAngle = (4.0*ENVIRONMENT_PI)/(6.0*sourceSize*sourceSize);
  let deepestLevel = environmentPass.parameters.y - 1.0;
  let basis = tangentBasis(normal);
  var sum = vec3f(0.0);
  var weight = 0.0;
  for(var index = 0u; index < count; index++) {
    let tangentHalf = ggxHalfVector(hammersley(index, count), alpha);
    let normalDotHalf = tangentHalf.z;
    let halfVector = basis*tangentHalf;
    let light = (2.0*normalDotHalf*halfVector) - normal;
    let normalDotLight = dot(normal, light);
    if(normalDotLight > 0.0) {
      // With the view along the normal, the light direction's density is the distribution over four.
      let density = ggxDistribution(normalDotHalf, alpha)*0.25;
      let sampleSolidAngle = 1.0/max(f32(count)*density, 0.000001);
      let level = clamp((0.5*log2(sampleSolidAngle/texelSolidAngle)) + 1.0, 0.0, deepestLevel);
      sum+= textureSampleLevel(sourceCube, environmentSampler, light, level).rgb*normalDotLight;
      weight+= normalDotLight;
    }
  }
  return vec4f(sum/max(weight, 0.000001), 1.0);
}
