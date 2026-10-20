// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// The shared full-screen vertex stage: one triangle covering the target, with top-left texture coordinates. Effect
// shaders compose this file first and take `FullscreenVarying` in their fragment entry points.

struct FullscreenVarying {
  @builtin(position) position: vec4f,
  @location(0) coordinate: vec2f,
};

@vertex fn vertexMain(@builtin(vertex_index) vertexIndex: u32) -> FullscreenVarying {
  let position = vec2f(f32((vertexIndex << 1u) & 2u), f32(vertexIndex & 2u));
  var output: FullscreenVarying;
  output.position = vec4f((position*2.0) - 1.0, 0.0, 1.0);
  output.coordinate = vec2f(position.x, 1.0 - position.y);
  return output;
}
