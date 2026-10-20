// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Motion blur reconstructed from motion vectors (after McGuire et al. 2012, "A Reconstruction Filter for Plausible
// Motion Blur"). Three passes: each tile finds the longest motion inside it, each tile takes the longest motion among
// its neighbors, and each pixel gathers samples along its tile's dominant motion, weighting them by depth and by how
// far each sample's own motion reaches, so fast foreground objects smear over the background without the background
// bleeding across them. Depth comes from the motion attachment's view-depth channel. Composed after fullscreen.wgsl.

struct MotionBlur {
  // XY scene size in pixels, Z shutter fraction, W longest blur in pixels.
  frame: vec4f,
  // X samples per pixel, Y tile size in pixels, Z soft depth range in view units, W unused.
  samples: vec4f,
  // Previous frame's view-projection times this frame's inverse (both unjittered), which reprojects the sky.
  skyReprojection: mat4x4f,
};

@group(0) @binding(0) var<uniform> blur: MotionBlur;
@group(0) @binding(1) var sceneTexture: texture_2d<f32>;
@group(0) @binding(2) var motionTexture: texture_2d<f32>;
@group(0) @binding(3) var tileTexture: texture_2d<f32>;
@group(0) @binding(4) var linearSampler: sampler;

/// View depth for pixels no surface covered.
const SKY_DEPTH: f32 = 1.0e6;

/**
 * A pixel's blur in pixels (motion over the exposure, capped at the longest blur) and its view depth.
 */
fn velocityAndDepth(pixel: vec2i) -> vec3f {
  let size = vec2i(blur.frame.xy);
  let clamped = clamp(pixel, vec2i(0), size - vec2i(1));
  let sample = textureLoad(motionTexture, clamped, 0);
  var motion = sample.xy;
  var depth = sample.z;
  if(sample.a < 0.5) {
    let coordinate = (vec2f(clamped) + vec2f(0.5))/blur.frame.xy;
    let ndc = vec2f((coordinate.x*2.0) - 1.0, 1.0 - (coordinate.y*2.0));
    let previous = blur.skyReprojection*vec4f(ndc, 1.0, 1.0);
    motion = vec2f(0.0);
    if(previous.w > 0.000001) {
      motion = coordinate - vec2f((previous.x/previous.w*0.5) + 0.5, 0.5 - (previous.y/previous.w*0.5));
    }
    depth = SKY_DEPTH;
  }
  var velocity = motion*blur.frame.xy*blur.frame.z;
  let magnitude = length(velocity);
  if(magnitude > blur.frame.w) {
    velocity = velocity*(blur.frame.w/magnitude);
  }
  return vec3f(velocity, depth);
}

/**
 * The longest blur inside each tile. Renders at tile resolution.
 */
@fragment fn tileMaxMain(input: FullscreenVarying) -> @location(0) vec4f {
  let tileSize = i32(blur.samples.y);
  let origin = vec2i(input.position.xy)*tileSize;
  var longest = vec2f(0.0);
  for(var y = 0; y < tileSize; y++) {
    for(var x = 0; x < tileSize; x++) {
      let velocity = velocityAndDepth(origin + vec2i(x, y)).xy;
      if(dot(velocity, velocity) > dot(longest, longest)) {
        longest = velocity;
      }
    }
  }
  return vec4f(longest, 0.0, 1.0);
}

/**
 * The longest blur among each tile and its eight neighbors. Renders at tile resolution.
 */
@fragment fn neighborMaxMain(input: FullscreenVarying) -> @location(0) vec4f {
  let tile = vec2i(input.position.xy);
  let lastTile = vec2i(textureDimensions(tileTexture)) - vec2i(1);
  var longest = vec2f(0.0);
  for(var y = -1; y <= 1; y++) {
    for(var x = -1; x <= 1; x++) {
      let velocity = textureLoad(tileTexture, clamp(tile + vec2i(x, y), vec2i(0), lastTile), 0).xy;
      if(dot(velocity, velocity) > dot(longest, longest)) {
        longest = velocity;
      }
    }
  }
  return vec4f(longest, 0.0, 1.0);
}

/**
 * One where `depth` lies in front of `reference` (view depths grow away from the eye), falling to zero over the soft
 * depth range behind it.
 */
fn inFront(depth: f32, reference: f32) -> f32 {
  return clamp(1.0 - ((depth - reference)/blur.samples.z), 0.0, 1.0);
}

/**
 * Weight of a sample `distance` pixels away whose blur reaches `reach` pixels, as a cone of its motion.
 */
fn cone(distance: f32, reach: f32) -> f32 {
  return clamp(1.0 - (distance/max(reach, 0.0001)), 0.0, 1.0);
}

/**
 * Weight of two samples blurring across each other.
 */
fn cylinder(distance: f32, reach: f32) -> f32 {
  return 1.0 - smoothstep(0.95*reach, 1.05*reach, distance);
}

/**
 * Interleaved gradient noise in [0, 1) (Jimenez 2014), offsetting sample positions per pixel.
 */
fn interleavedGradientNoise(pixel: vec2f) -> f32 {
  return fract(52.9829189*fract(dot(pixel, vec2f(0.06711056, 0.00583715))));
}

/**
 * Gather samples along the tile's dominant blur. Renders at scene resolution.
 */
@fragment fn gatherMain(input: FullscreenVarying) -> @location(0) vec4f {
  let pixel = vec2i(input.position.xy);
  let color = textureLoad(sceneTexture, pixel, 0).rgb;
  let tile = pixel/i32(blur.samples.y);
  let dominant = textureLoad(tileTexture, min(tile, vec2i(textureDimensions(tileTexture)) - vec2i(1)), 0).xy;
  let dominantLength = length(dominant);
  if(dominantLength < 0.5) {
    return vec4f(color, 1.0);
  }

  let center = velocityAndDepth(pixel);
  let centerReach = max(length(center.xy), 0.5);
  var total = 1.0/centerReach;
  var sum = color*total;
  let count = i32(blur.samples.x);
  let jitter = interleavedGradientNoise(input.position.xy) - 0.5;
  for(var index = 0; index < count; index++) {
    // Samples span the blur in both directions, offset per pixel so banding becomes noise.
    let t = mix(-0.5, 0.5, (f32(index) + 0.5 + jitter)/f32(count));
    if(abs(t) < 0.0001) {
      continue;
    }
    let offset = dominant*t;
    let samplePixel = vec2i(input.position.xy + offset);
    let sample = velocityAndDepth(samplePixel);
    let distance = length(offset);
    let sampleReach = max(length(sample.xy), 0.5);

    // Foreground samples whose blur reaches this pixel, this pixel's own blur over the background behind it, and both
    // blurring together.
    let foreground = inFront(sample.z, center.z)*cone(distance, sampleReach);
    let background = inFront(center.z, sample.z)*cone(distance, centerReach);
    let both = cylinder(distance, sampleReach)*cylinder(distance, centerReach)*2.0;
    let weight = foreground + background + both;
    let coordinate = (vec2f(samplePixel) + vec2f(0.5))/blur.frame.xy;
    sum+= textureSampleLevel(sceneTexture, linearSampler, coordinate, 0.0).rgb*weight;
    total+= weight;
  }
  return vec4f(sum/total, 1.0);
}
