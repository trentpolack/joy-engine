// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Adaptive exposure with compute: a 256-bin histogram of the scene's log luminance (every other pixel on each axis,
// weighted toward the center), then one invocation averages the bins between two percentiles, sets the exposure that
// maps that average to the key value, and moves the previous exposure toward it at the brightening or darkening rate.
// Bin zero counts black pixels, which never meter. The result is written to a one-texel texture the display chain
// reads.

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

const BIN_COUNT: u32 = 256u;

@group(0) @binding(0) var<uniform> exposure: Exposure;
@group(0) @binding(1) var sceneTexture: texture_2d<f32>;
@group(0) @binding(2) var<storage, read_write> histogram: array<atomic<u32>, 256>;
@group(0) @binding(3) var<storage, read_write> state: array<f32, 4>;
@group(0) @binding(4) var exposureOutput: texture_storage_2d<rgba16float, write>;

var<workgroup> localBins: array<atomic<u32>, 256>;

/**
 * The bin of a luminance: zero for black, else its log2 position in the metered range.
 */
fn luminanceBin(luminance: f32) -> u32 {
  if(luminance < 0.00001) {
    return 0u;
  }
  let position = clamp((log2(luminance) - exposure.metering.x)/exposure.metering.y, 0.0, 1.0);
  return 1u + u32(position*f32(BIN_COUNT - 2u));
}

/**
 * Count one workgroup's pixels into shared bins, then add them to the frame's histogram.
 */
@compute @workgroup_size(16, 16)
fn histogramMain(@builtin(global_invocation_id) id: vec3u, @builtin(local_invocation_index) index: u32) {
  atomicStore(&localBins[index], 0u);
  workgroupBarrier();

  let size = textureDimensions(sceneTexture);
  let pixel = id.xy*2u;
  if(all(pixel < size)) {
    let color = textureLoad(sceneTexture, vec2i(pixel), 0).rgb;
    let luminance = dot(color, vec3f(0.2126, 0.7152, 0.0722));
    // Center weighting: one everywhere at zero weight, falling toward the corners as the weight rises.
    let fromCenter = length((vec2f(pixel)/vec2f(size)) - vec2f(0.5))*1.41421356;
    let weight = mix(1.0, max(1.0 - fromCenter, 0.05), exposure.metering.z);
    atomicAdd(&localBins[luminanceBin(luminance)], u32((weight*16.0) + 0.5));
  }
  workgroupBarrier();
  atomicAdd(&histogram[index], atomicLoad(&localBins[index]));
}

/**
 * Average the bins between the percentiles, adapt toward the matching exposure, and clear the histogram.
 */
@compute @workgroup_size(1)
fn adaptMain() {
  var total = 0.0;
  for(var bin = 1u; bin < BIN_COUNT; bin++) {
    total+= f32(atomicLoad(&histogram[bin]));
  }
  let lowCount = total*exposure.average.x;
  let highCount = total*exposure.average.y;
  var seen = 0.0;
  var weightedLog = 0.0;
  var counted = 0.0;
  for(var bin = 1u; bin < BIN_COUNT; bin++) {
    let count = f32(atomicLoad(&histogram[bin]));
    // The part of this bin's count that lies between the two percentiles.
    let inside = max(min(seen + count, highCount) - max(seen, lowCount), 0.0);
    let binLog = exposure.metering.x + (((f32(bin - 1u) + 0.5)/f32(BIN_COUNT - 2u))*exposure.metering.y);
    weightedLog+= binLog*inside;
    counted+= inside;
    seen+= count;
  }
  for(var bin = 0u; bin < BIN_COUNT; bin++) {
    atomicStore(&histogram[bin], 0u);
  }

  var previous = state[0];
  if((exposure.limits.w > 0.5) || !(previous > 0.0)) {
    previous = exposure.limits.z;
  }
  var targetExposure = previous;
  if(counted > 0.0) {
    let average = exp2(weightedLog/counted);
    targetExposure = clamp(exposure.average.z/average, exposure.limits.x, exposure.limits.y);
  }
  var rate = exposure.speed.y;
  if(targetExposure > previous) {
    rate = exposure.speed.x;
  }
  let blend = 1.0 - exp(-rate*exposure.average.w);
  let adapted = exp2(mix(log2(previous), log2(targetExposure), blend));
  state[0] = adapted;
  textureStore(exposureOutput, vec2i(0), vec4f(adapted, 0.0, 0.0, 1.0));
}
