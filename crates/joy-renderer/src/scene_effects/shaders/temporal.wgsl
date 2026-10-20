// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Temporal antialiasing resolve. The camera jitters by a sub-pixel offset every frame; each pixel blends this frame's
// sample with its history, reprojected by the scene's motion vectors and clipped to the variance of this frame's
// neighborhood so disoccluded and changing content does not ghost. Colors are compressed by their brightness while
// they are compared and blended, so a few very bright pixels cannot dominate and flicker. Composed after
// fullscreen.wgsl.

struct Temporal {
  // XY texel size in UV, Z history weight, W 1 when the history is valid (zero after a reset).
  frame: vec4f,
  // Previous frame's view-projection times this frame's inverse (both unjittered), which reprojects the sky.
  skyReprojection: mat4x4f,
};

@group(0) @binding(0) var<uniform> temporal: Temporal;
@group(0) @binding(1) var sceneTexture: texture_2d<f32>;
@group(0) @binding(2) var historyTexture: texture_2d<f32>;
@group(0) @binding(3) var motionTexture: texture_2d<f32>;
@group(0) @binding(4) var linearSampler: sampler;

/**
 * Brightness compression and its inverse: `c / (1 + max(c))`, which keeps HDR highlights from dominating blends.
 */
fn compress(color: vec3f) -> vec3f {
  return color/(1.0 + max(color.r, max(color.g, color.b)));
}

fn expand(color: vec3f) -> vec3f {
  return color/max(1.0 - max(color.r, max(color.g, color.b)), 0.0001);
}

fn toYcocg(color: vec3f) -> vec3f {
  return vec3f(
    dot(color, vec3f(0.25, 0.5, 0.25)),
    dot(color, vec3f(0.5, 0.0, -0.5)),
    dot(color, vec3f(-0.25, 0.5, -0.25))
  );
}

fn fromYcocg(color: vec3f) -> vec3f {
  return vec3f(color.x + color.y - color.z, color.x + color.z, color.x - color.y - color.z);
}

/**
 * Motion of a sky pixel, which no surface wrote: its far-plane point reprojected through the camera change.
 */
fn skyMotion(coordinate: vec2f) -> vec2f {
  let ndc = vec2f((coordinate.x*2.0) - 1.0, 1.0 - (coordinate.y*2.0));
  let previous = temporal.skyReprojection*vec4f(ndc, 1.0, 1.0);
  if(previous.w <= 0.000001) {
    return vec2f(0.0);
  }
  let previousCoordinate = vec2f((previous.x/previous.w*0.5) + 0.5, 0.5 - (previous.y/previous.w*0.5));
  return coordinate - previousCoordinate;
}

/**
 * History at a coordinate through a Catmull-Rom filter, which keeps it sharp across many reprojections; nine bilinear
 * fetches cover its sixteen taps.
 */
fn sampleHistory(coordinate: vec2f) -> vec3f {
  let texel = temporal.frame.xy;
  let position = (coordinate/texel) - vec2f(0.5);
  let origin = floor(position);
  let fraction = position - origin;
  let weight0 = fraction*(-0.5 + (fraction*(1.0 - (0.5*fraction))));
  let weight1 = 1.0 + ((fraction*fraction)*((1.5*fraction) - 2.5));
  let weight2 = fraction*(0.5 + (fraction*(2.0 - (1.5*fraction))));
  let weight3 = (fraction*fraction)*((0.5*fraction) - 0.5);
  let weightMiddle = weight1 + weight2;
  let coordinateBefore = (origin - vec2f(0.5))*texel;
  let coordinateMiddle = (origin + vec2f(0.5) + (weight2/weightMiddle))*texel;
  let coordinateAfter = (origin + vec2f(2.5))*texel;

  let rows = array<f32, 3>(coordinateBefore.y, coordinateMiddle.y, coordinateAfter.y);
  let rowWeights = array<f32, 3>(weight0.y, weightMiddle.y, weight3.y);
  let columns = array<f32, 3>(coordinateBefore.x, coordinateMiddle.x, coordinateAfter.x);
  let columnWeights = array<f32, 3>(weight0.x, weightMiddle.x, weight3.x);
  var color = vec3f(0.0);
  var totalWeight = 0.0;
  for(var row = 0u; row < 3u; row++) {
    for(var column = 0u; column < 3u; column++) {
      let weight = rowWeights[row]*columnWeights[column];
      color+= textureSampleLevel(historyTexture, linearSampler, vec2f(columns[column], rows[row]), 0.0).rgb*weight;
      totalWeight+= weight;
    }
  }
  return max(color/totalWeight, vec3f(0.0));
}

/**
 * Move `color` toward the box center until it lies inside the box.
 */
fn clipToBox(color: vec3f, boxMinimum: vec3f, boxMaximum: vec3f) -> vec3f {
  let center = 0.5*(boxMaximum + boxMinimum);
  let extent = (0.5*(boxMaximum - boxMinimum)) + vec3f(0.00001);
  let offset = color - center;
  let units = abs(offset/extent);
  let largest = max(units.x, max(units.y, units.z));
  if(largest > 1.0) {
    return center + (offset/largest);
  }
  return color;
}

@fragment fn fragmentMain(input: FullscreenVarying) -> @location(0) vec4f {
  let pixel = vec2i(input.position.xy);
  let lastPixel = vec2i(textureDimensions(sceneTexture)) - vec2i(1);
  let current = textureLoad(sceneTexture, pixel, 0).rgb;
  if(temporal.frame.w < 0.5) {
    return vec4f(current, 1.0);
  }

  // The neighborhood's mean and spread in compressed YCoCg, a box the history must fall inside.
  var sum = vec3f(0.0);
  var sumSquares = vec3f(0.0);
  var minimum = vec3f(1.0e9);
  var maximum = vec3f(-1.0e9);
  for(var y = -1; y <= 1; y++) {
    for(var x = -1; x <= 1; x++) {
      let neighbor = clamp(pixel + vec2i(x, y), vec2i(0), lastPixel);
      let value = toYcocg(compress(textureLoad(sceneTexture, neighbor, 0).rgb));
      sum+= value;
      sumSquares+= value*value;
      minimum = min(minimum, value);
      maximum = max(maximum, value);
    }
  }
  let mean = sum/9.0;
  let deviation = sqrt(max((sumSquares/9.0) - (mean*mean), vec3f(0.0)));
  let boxMinimum = max(minimum, mean - (1.25*deviation));
  let boxMaximum = min(maximum, mean + (1.25*deviation));

  // Reproject: surfaces carry motion; the sky moves only with the camera.
  let motionSample = textureLoad(motionTexture, pixel, 0);
  var motion = motionSample.xy;
  if(motionSample.a < 0.5) {
    motion = skyMotion(input.coordinate);
  }
  let previousCoordinate = input.coordinate - motion;
  if(any(previousCoordinate < vec2f(0.0)) || any(previousCoordinate > vec2f(1.0))) {
    return vec4f(current, 1.0);
  }
  let history = toYcocg(compress(sampleHistory(previousCoordinate)));
  let clipped = expand(fromYcocg(clipToBox(history, boxMinimum, boxMaximum)));

  // Blend, weighting each side by its inverse brightness.
  let historyWeight = temporal.frame.z;
  let currentShare = (1.0 - historyWeight)/(1.0 + dot(current, vec3f(0.2126, 0.7152, 0.0722)));
  let historyShare = historyWeight/(1.0 + dot(clipped, vec3f(0.2126, 0.7152, 0.0722)));
  return vec4f(((current*currentShare) + (clipped*historyShare))/(currentShare + historyShare), 1.0);
}
