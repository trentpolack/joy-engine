// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Deferred lighting for one shading model: a full-screen triangle whose stencil test passes only the pixels the
// model's surfaces wrote in the G-buffer pass. Each pixel rebuilds its world position from the scene depth, decodes
// its surface with the model's `materialDecode`, and adds the engine's direct light loop (surface-lighting.wgsl) to
// the scene color, which the pipeline blends additively. Composed after scene-common.wgsl, shadow-receiver.wgsl,
// scene-vertex.wgsl, surface-contract.wgsl, gbuffer-encoding.wgsl, environment-lighting.wgsl, the model, and
// surface-lighting.wgsl.
//
// The pass binds only the frame group (with the depth and G-buffer below) and the shadow group, so `materialDecode`
// and `materialDirect` must not read the model's material group or image lighting.

@group(0) @binding(6) var sceneDepth: texture_depth_2d;
@group(0) @binding(7) var gbuffer: texture_2d<u32>;

/**
 * One triangle covering the target.
 */
@vertex fn lightingVertexMain(@builtin(vertex_index) vertexIndex: u32) -> @builtin(position) vec4f {
  let corner = vec2f(f32((vertexIndex << 1u) & 2u), f32(vertexIndex & 2u));
  return vec4f((corner*2.0) - 1.0, 0.0, 1.0);
}

/**
 * Direct light reaching one G-buffer pixel.
 */
@fragment fn lightingFragmentMain(@builtin(position) position: vec4f) -> @location(0) vec4f {
  let pixel = vec2i(position.xy);
  let depth = textureLoad(sceneDepth, pixel, 0);
  let texel = textureLoad(gbuffer, pixel, 0);

  // The rasterized depth came from the frame's (possibly jittered) projection, which the inverse undoes.
  let coordinate = position.xy*frame.viewport.zw;
  let clip = vec4f((coordinate.x*2.0) - 1.0, 1.0 - (coordinate.y*2.0), depth, 1.0);
  let world = frame.inverseViewProjection*clip;
  let worldPosition = world.xyz/world.w;

  let normal = gbufferNormal(texel.x);
  let view = normalize(frame.eye.xyz - worldPosition);
  let surface = materialDecode(texel.yzw, normal, view);
  return vec4f(directLighting(surface, worldPosition, normal, gbufferReceivesShadow(texel.x)), 0.0);
}
