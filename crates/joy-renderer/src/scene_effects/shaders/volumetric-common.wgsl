// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Volumetric fog's froxel grid: cells aligned with the screen and sliced along view depth, with slices spaced by the
// square root of depth so near slices are thin (`fogAt` in scene-common.wgsl reads the same mapping). Shared by the
// injection and integration passes.

struct Volumetric {
  // This frame's unjittered inverse projection and inverse view, which place froxels in the world.
  inverseProjection: mat4x4f,
  inverseView: mat4x4f,
  // The previous frame's unjittered view-projection and view, which reproject froxels into its volume.
  previousViewProjection: mat4x4f,
  previousView: mat4x4f,
  // XYZ grid size in froxels, W the view depth the volume covers.
  grid: vec4f,
  // X extinction per world unit at the base height, Y height falloff, Z base height, W anisotropy.
  medium: vec4f,
  // RGB albedo, W history weight (zero without valid history).
  albedo: vec4f,
  // RGB emission per world unit at the base density, W this frame's depth jitter in [0, 1).
  emissive: vec4f,
  // X key light, Y point light, Z skylight intensity; W 1 when lights cast volumetric shadows.
  intensity: vec4f,
};

/**
 * View depth at a slice coordinate in [0, 1].
 */
fn sliceDepth(slice: f32, maxDepth: f32) -> f32 {
  return maxDepth*slice*slice;
}

/**
 * The view-space point at `viewDepth` along the ray through a screen coordinate (top-left origin). Works for
 * perspective and orthographic projections alike by interpolating between the near and far planes.
 */
fn viewPositionAt(inverseProjection: mat4x4f, coordinate: vec2f, viewDepth: f32) -> vec3f {
  let ndc = vec2f((coordinate.x*2.0) - 1.0, 1.0 - (coordinate.y*2.0));
  let nearPoint = inverseProjection*vec4f(ndc, 0.0, 1.0);
  let farPoint = inverseProjection*vec4f(ndc, 1.0, 1.0);
  let nearView = nearPoint.xyz/nearPoint.w;
  let farView = farPoint.xyz/farPoint.w;
  let t = (viewDepth + nearView.z)/(nearView.z - farView.z);
  return mix(nearView, farView, t);
}
