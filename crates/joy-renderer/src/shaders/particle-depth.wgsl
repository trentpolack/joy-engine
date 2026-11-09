// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Soft particles: the scene depth a particle fades against. Composed after scene-common.wgsl and particle-shapes.wgsl
// and before particle-scene.wgsl, for the soft pipelines of devices that can sample the depth buffer while it is
// attached read-only (every backend but WebGL 2, which composes particle-depth-none.wgsl instead). Group 3 is the scene
// depth the opaque passes wrote, bound only while transparency draws in its own pass.

@group(3) @binding(0) var particleSceneDepth: texture_depth_2d;

/**
 * How much of a fragment survives near opaque geometry: zero where it touches the nearest opaque surface behind it,
 * rising linearly to one at the instance's softness distance (world units along the view) in front of it. Instances
 * without softness, and fragments with no opaque surface behind them (depth cleared to the far plane), keep everything.
 */
fn particleSceneFade(input: ParticleVarying) -> f32 {
  let softness = input.parameters.w;
  if(softness <= 0.0) {
    return 1.0;
  }

  // Rebuild the opaque surface behind this pixel from the depth buffer, as deferred lighting does; the rasterized depth
  // came from the frame's (possibly jittered) projection, which the inverse undoes.
  let depth = textureLoad(particleSceneDepth, vec2i(input.clip.xy), 0);
  let coordinate = input.clip.xy*frame.viewport.zw;
  let clip = vec4f((coordinate.x*2.0) - 1.0, 1.0 - (coordinate.y*2.0), depth, 1.0);
  let world = frame.inverseViewProjection*clip;
  let sceneDepth = -(frame.view*vec4f(world.xyz/world.w, 1.0)).z;
  let particleDepth = -(frame.view*vec4f(input.worldPosition, 1.0)).z;
  return clamp((sceneDepth - particleDepth)/softness, 0.0, 1.0);
}
