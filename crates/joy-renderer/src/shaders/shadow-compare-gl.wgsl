// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Shadow comparisons for the GL backend, replacing the hardware versions in shadow-receiver.wgsl. WebGL 2 cannot sample
// depth arrays with an explicit level in comparison mode, so these read the four nearest depths (through the receiver's
// nearest sampler) and filter the comparisons bilinearly, which is what a linear comparison sampler does.

fn compareCascade(coordinate: vec2f, cascade: u32, reference: f32) -> f32 {
  let size = vec2i(textureDimensions(cascadeDepths).xy);
  let texel = (coordinate*vec2f(size)) - vec2f(0.5);
  let base = vec2i(floor(texel));
  let weight = fract(texel);
  let high = size - vec2i(1);
  let lit00 = step(reference, cascadeDepth(clamp(base, vec2i(0), high), cascade));
  let lit10 = step(reference, cascadeDepth(clamp(base + vec2i(1, 0), vec2i(0), high), cascade));
  let lit01 = step(reference, cascadeDepth(clamp(base + vec2i(0, 1), vec2i(0), high), cascade));
  let lit11 = step(reference, cascadeDepth(clamp(base + vec2i(1, 1), vec2i(0), high), cascade));
  return mix(mix(lit00, lit10, weight.x), mix(lit01, lit11, weight.x), weight.y);
}

fn compareAtlas(coordinate: vec2f, reference: f32) -> f32 {
  let size = vec2i(textureDimensions(pointShadowDepths));
  let texel = (coordinate*vec2f(size)) - vec2f(0.5);
  let base = vec2i(floor(texel));
  let weight = fract(texel);
  let high = size - vec2i(1);
  let lit00 = step(reference, atlasDepth(clamp(base, vec2i(0), high)));
  let lit10 = step(reference, atlasDepth(clamp(base + vec2i(1, 0), vec2i(0), high)));
  let lit01 = step(reference, atlasDepth(clamp(base + vec2i(0, 1), vec2i(0), high)));
  let lit11 = step(reference, atlasDepth(clamp(base + vec2i(1, 1), vec2i(0), high)));
  return mix(mix(lit00, lit10, weight.x), mix(lit01, lit11, weight.x), weight.y);
}
