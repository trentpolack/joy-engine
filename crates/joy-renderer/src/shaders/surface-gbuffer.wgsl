// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// G-buffer entry points for shading models that support deferred lighting (they implement `materialEncode` and
// `materialDecode`). Composed after surface-scene.wgsl, whose vertex stage and fragment helpers these share.
//
// Each opaque fragment writes the light that does not depend on the frame's direct lights (emission, ambient,
// skylight, and image-based light) into the scene color, and its encoded surface into the G-buffer; the deferred
// lighting pass for its model adds direct light afterwards. The pipeline writes the model's lighting slot into the
// stencil buffer, so that pass shades only this model's pixels.

struct GBufferTargets {
  @location(0) color: vec4f,
  @location(1) gbuffer: vec4u,
};

struct GBufferEffectTargets {
  @location(0) color: vec4f,
  // See `SceneTargets` in scene-common.wgsl.
  @location(1) motion: vec4f,
  @location(2) gbuffer: vec4u,
};

/**
 * Scene color and G-buffer.
 */
@fragment fn fragmentGBuffer(interpolated: SurfaceVarying, @builtin(front_facing) frontFacing: bool) -> GBufferTargets {
  let input = surfaceInput(interpolated, frontFacing);
  let surface = materialSurface(input);
  alphaTest(materialAlpha(surface));

  var targets: GBufferTargets;
  targets.color = vec4f(undirectedLight(surface), 1.0);
  targets.gbuffer = gbufferTexel(surface, interpolated.receivesShadow);
  return targets;
}

/**
 * Scene color, the scene-effect motion attachment, and G-buffer.
 */
@fragment fn fragmentGBufferTargets(
  interpolated: SurfaceVarying,
  @builtin(front_facing) frontFacing: bool
) -> GBufferEffectTargets {
  let input = surfaceInput(interpolated, frontFacing);
  let surface = materialSurface(input);
  alphaTest(materialAlpha(surface));

  let clips = mat2x4f(interpolated.currentClip, interpolated.previousClip);
  let scene = sceneTargets(vec4f(undirectedLight(surface), 1.0), interpolated.worldPosition, clips);
  var targets: GBufferEffectTargets;
  targets.color = scene.color;
  targets.motion = scene.motion;
  targets.gbuffer = gbufferTexel(surface, interpolated.receivesShadow);
  return targets;
}

/**
 * Emission plus ambient and image-based light: everything but the frame's direct lights.
 */
fn undirectedLight(surface: MaterialSurface) -> vec3f {
  var radiance = materialEmission(surface);
  if(MATERIAL_RECEIVES_LIGHT) {
    radiance+= materialIndirect(surface, ambientLight());
  }
  return radiance;
}

/**
 * The engine header (shading normal and shadow flag) followed by the model's encoding.
 */
fn gbufferTexel(surface: MaterialSurface, receivesShadow: f32) -> vec4u {
  let header = gbufferHeader(materialAttributes(surface).normal, receivesShadow > 0.5);
  return vec4u(header, materialEncode(surface));
}
