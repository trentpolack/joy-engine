// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Buffer Overview tiles: each draw covers one grid tile of the output (its viewport) and shows one frame buffer
// scaled to the tile. Color tiles sample a finished frame; the depth and motion tiles visualize the lit frame's scene
// depth and motion vectors. Composed after fullscreen.wgsl.

struct OverviewParameters {
  // Camera near and far planes in world units, and the depth tile's display range in meters.
  depthRange: vec4f,
};

@group(0) @binding(0) var overviewColor: texture_2d<f32>;
@group(0) @binding(1) var overviewSampler: sampler;
@group(0) @binding(2) var overviewDepth: texture_depth_2d;
@group(0) @binding(3) var overviewMotion: texture_2d<f32>;
@group(0) @binding(4) var<uniform> overview: OverviewParameters;

/**
 * A finished frame, scaled into the tile.
 */
@fragment fn colorTile(input: FullscreenVarying) -> @location(0) vec4f {
  return vec4f(textureSample(overviewColor, overviewSampler, input.coordinate).rgb, 1.0);
}

/**
 * Scene depth as linear view distance, near white and far black on a logarithmic ramp, with the sky black.
 */
@fragment fn depthTile(input: FullscreenVarying) -> @location(0) vec4f {
  let size = vec2f(textureDimensions(overviewDepth));
  let pixel = vec2i(min(input.coordinate*size, size - vec2f(1.0)));
  let depth = textureLoad(overviewDepth, pixel, 0);
  let near = overview.depthRange.x;
  let far = overview.depthRange.y;
  if(depth >= 1.0) {
    return vec4f(0.0, 0.0, 0.0, 1.0);
  }
  // Zero-to-one perspective depth back to view distance.
  let distance = (near*far)/(far - (depth*(far - near)));
  let shade = 1.0 - clamp(log2(1.0 + distance)/log2(1.0 + overview.depthRange.z), 0.0, 1.0);
  return vec4f(vec3f(shade), 1.0);
}

/**
 * Screen-space motion: direction as hue-like red and green around mid gray, scaled so a tenth of the screen per frame
 * saturates; blue marks pixels without a written vector.
 */
@fragment fn motionTile(input: FullscreenVarying) -> @location(0) vec4f {
  let size = vec2f(textureDimensions(overviewMotion));
  let pixel = vec2i(min(input.coordinate*size, size - vec2f(1.0)));
  let motion = textureLoad(overviewMotion, pixel, 0);
  if(motion.a < 0.5) {
    return vec4f(0.05, 0.05, 0.15, 1.0);
  }
  let scaled = clamp(motion.xy*10.0, vec2f(-1.0), vec2f(1.0));
  return vec4f((scaled*0.5) + 0.5, 0.5, 1.0);
}
