// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// UI composite for extended linear (scRGB) presentation targets. egui paints into an sRGB-format intermediate, which
// stores its premultiplied colors encoded and returns them as linear light when loaded; this pass scales that light so
// UI white lands on the display's SDR white, then blends it over the scene with premultiplied alpha.

struct Composite {
  // X linear signal value of SDR white, YZW unused.
  white: vec4f,
};

@group(0) @binding(0) var<uniform> composite: Composite;
@group(0) @binding(1) var interfaceTexture: texture_2d<f32>;

/**
 * Full-screen triangle.
 */
@vertex fn vertexMain(@builtin(vertex_index) vertexIndex: u32) -> @builtin(position) vec4f {
  let position = vec2f(f32((vertexIndex << 1u) & 2u), f32(vertexIndex & 2u));
  return vec4f((position*2.0) - 1.0, 0.0, 1.0);
}

/**
 * One UI pixel, read texel for texel (the intermediate matches the presentation size).
 */
@fragment fn fragmentMain(@builtin(position) position: vec4f) -> @location(0) vec4f {
  let interface = textureLoad(interfaceTexture, vec2i(position.xy), 0);
  return vec4f(interface.rgb*composite.white.x, interface.a);
}
