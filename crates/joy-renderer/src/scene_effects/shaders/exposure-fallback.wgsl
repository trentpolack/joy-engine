// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Adaptive exposure without compute (WebGL 2): the scene's center-weighted log luminance is metered into a small
// targetExposure, reduced to one texel, and one draw moves the previous exposure toward the exposure that maps the average to
// the key value. Without a histogram it cannot ignore percentiles, so small bright lights sway it more than the
// compute path. Composed after fullscreen.wgsl.

struct Exposure {
  // X lowest metered log2 luminance, Y log2 range, Z center weight, W unused.
  metering: vec4f,
  // X low percentile, Y high percentile, Z key value, W seconds since the previous frame.
  average: vec4f,
  // X lowest exposure, Y highest exposure, Z initial exposure, W 1 to restart from the initial exposure.
  limits: vec4f,
  // X brightening rate, Y darkening rate (per second), ZW unused.
  speed: vec4f,
};

@group(0) @binding(0) var<uniform> exposure: Exposure;
@group(0) @binding(1) var sourceTexture: texture_2d<f32>;
@group(0) @binding(2) var previousTexture: texture_2d<f32>;
@group(0) @binding(3) var linearSampler: sampler;

// Size of the first reduction targetExposure (`METER_SIZE` in exposure.rs); three four-to-one reductions bring it to one.
const METER_SIZE: u32 = 64u;

/**
 * Meter the scene into the first reduction targetExposure: four scene taps per texel, as weighted log luminance (X) and weight
 * (Y).
 */
@fragment fn meterMain(input: FullscreenVarying) -> @location(0) vec4f {
  // Taps a quarter of a meter texel apart, inside its footprint.
  let step = vec2f(0.25/f32(METER_SIZE));
  var logSum = 0.0;
  for(var index = 0u; index < 4u; index++) {
    let offset = (vec2f(f32(index & 1u), f32(index >> 1u)) - vec2f(0.5))*step;
    let color = textureSampleLevel(sourceTexture, linearSampler, input.coordinate + offset, 0.0).rgb;
    let luminance = dot(color, vec3f(0.2126, 0.7152, 0.0722));
    logSum+= clamp(log2(max(luminance, 0.00001)), exposure.metering.x, exposure.metering.x + exposure.metering.y);
  }
  let fromCenter = length(input.coordinate - vec2f(0.5))*1.41421356;
  let weight = mix(1.0, max(1.0 - fromCenter, 0.05), exposure.metering.z);
  return vec4f((logSum*0.25)*weight, weight, 0.0, 1.0);
}

/**
 * Average four-by-four texels of the previous reduction.
 */
@fragment fn reduceMain(input: FullscreenVarying) -> @location(0) vec4f {
  let origin = vec2i(input.position.xy)*4;
  let last = vec2i(textureDimensions(sourceTexture)) - vec2i(1);
  var sum = vec2f(0.0);
  for(var y = 0; y < 4; y++) {
    for(var x = 0; x < 4; x++) {
      sum+= textureLoad(sourceTexture, min(origin + vec2i(x, y), last), 0).xy;
    }
  }
  return vec4f(sum/16.0, 0.0, 1.0);
}

/**
 * Adapt the previous exposure toward the metered targetExposure. Renders one texel.
 */
@fragment fn adaptMain(input: FullscreenVarying) -> @location(0) vec4f {
  let metered = textureLoad(sourceTexture, vec2i(0), 0).xy;
  var previous = textureLoad(previousTexture, vec2i(0), 0).r;
  if((exposure.limits.w > 0.5) || !(previous > 0.0)) {
    previous = exposure.limits.z;
  }
  var targetExposure = previous;
  if(metered.y > 0.0) {
    let average = exp2(metered.x/metered.y);
    targetExposure = clamp(exposure.average.z/average, exposure.limits.x, exposure.limits.y);
  }
  var rate = exposure.speed.y;
  if(targetExposure > previous) {
    rate = exposure.speed.x;
  }
  let blend = 1.0 - exp(-rate*exposure.average.w);
  return vec4f(exp2(mix(log2(previous), log2(targetExposure), blend)), 0.0, 0.0, 1.0);
}
