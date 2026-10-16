// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Shadow comparisons for the GL backend, replacing the hardware versions in scene-common.wgsl. WebGL 2 cannot sample
// depth arrays with an explicit level in comparison mode, so these fetch the four nearest depths and filter the
// comparisons bilinearly, which is what a linear comparison sampler does.

fn compareDirectionalShadow(coordinate: vec2f, layer: i32, reference: f32) -> f32 {
  let size = vec2i(textureDimensions(directionalShadowDepths));
  let texel = (coordinate*vec2f(size)) - vec2f(0.5);
  let base = vec2i(floor(texel));
  let weight = fract(texel);
  let high = size - vec2i(1);
  let lit00 = step(reference, textureLoad(directionalShadowDepths, clamp(base, vec2i(0), high), layer, 0).x);
  let lit10 = step(reference, textureLoad(directionalShadowDepths, clamp(base + vec2i(1, 0), vec2i(0), high), layer, 0).x);
  let lit01 = step(reference, textureLoad(directionalShadowDepths, clamp(base + vec2i(0, 1), vec2i(0), high), layer, 0).x);
  let lit11 = step(reference, textureLoad(directionalShadowDepths, clamp(base + vec2i(1, 1), vec2i(0), high), layer, 0).x);
  return mix(mix(lit00, lit10, weight.x), mix(lit01, lit11, weight.x), weight.y);
}

fn comparePointShadow(coordinate: vec2f, layer: i32, reference: f32) -> f32 {
  let size = vec2i(textureDimensions(pointShadowDepths));
  let texel = (coordinate*vec2f(size)) - vec2f(0.5);
  let base = vec2i(floor(texel));
  let weight = fract(texel);
  let high = size - vec2i(1);
  let lit00 = step(reference, textureLoad(pointShadowDepths, clamp(base, vec2i(0), high), layer, 0).x);
  let lit10 = step(reference, textureLoad(pointShadowDepths, clamp(base + vec2i(1, 0), vec2i(0), high), layer, 0).x);
  let lit01 = step(reference, textureLoad(pointShadowDepths, clamp(base + vec2i(0, 1), vec2i(0), high), layer, 0).x);
  let lit11 = step(reference, textureLoad(pointShadowDepths, clamp(base + vec2i(1, 1), vec2i(0), high), layer, 0).x);
  return mix(mix(lit00, lit10, weight.x), mix(lit01, lit11, weight.x), weight.y);
}
