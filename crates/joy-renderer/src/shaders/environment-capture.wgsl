// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// The radiance cube the specular prefilter samples. `equirectMain` resamples the equirectangular source into a face of
// the sharpest level, averaging a grid of taps per texel so a source much larger than the cube (a sun a few pixels
// wide) keeps its energy instead of falling between samples; `downsampleMain` halves the previous level, whose four
// texels under each new texel a single bilinear tap averages. Composed after environment-common.wgsl.

@group(0) @binding(1) var equirectTexture: texture_2d<f32>;
@group(0) @binding(2) var environmentSampler: sampler;
@group(0) @binding(3) var sourceCube: texture_cube<f32>;

/**
 * Equirectangular radiance in a direction: U wraps around Y from +X, V runs from straight up to straight down.
 */
fn equirectRadiance(direction: vec3f) -> vec3f {
  let unit = normalize(direction);
  let azimuth = atan2(unit.z, unit.x)/(2.0*ENVIRONMENT_PI);
  let coordinate = vec2f(azimuth + 0.5, acos(clamp(unit.y, -1.0, 1.0))/ENVIRONMENT_PI);
  var color = textureSampleLevel(equirectTexture, environmentSampler, coordinate, 0.0).rgb;
  if(environmentPass.parameters.z > 0.5) {
    color = select(color/12.92, pow((color + 0.055)/1.055, vec3f(2.4)), color >= vec3f(0.04045));
  }
  return color;
}

@fragment fn equirectMain(@builtin(position) position: vec4f) -> @location(0) vec4f {
  let face = u32(environmentPass.destination.x);
  let size = environmentPass.destination.y;
  let taps = max(u32(environmentPass.parameters.w), 1u);
  var sum = vec3f(0.0);
  for(var y = 0u; y < taps; y++) {
    for(var x = 0u; x < taps; x++) {
      let offset = ((vec2f(f32(x), f32(y)) + vec2f(0.5))/f32(taps)) - vec2f(0.5);
      sum+= equirectRadiance(faceDirection(face, (position.xy + offset)/size));
    }
  }
  return vec4f(sum/f32(taps*taps), 1.0);
}

@fragment fn downsampleMain(@builtin(position) position: vec4f) -> @location(0) vec4f {
  let face = u32(environmentPass.destination.x);
  let direction = faceDirection(face, position.xy/environmentPass.destination.y);
  return vec4f(textureSampleLevel(sourceCube, environmentSampler, direction, 0.0).rgb, 1.0);
}
