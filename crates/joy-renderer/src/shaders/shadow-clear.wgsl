// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Clears one tile of the point-light shadow atlas to the far plane before the tile renders again. The atlas keeps
// every other light's cached faces, so the pass loads it instead of clearing it, and each re-rendered tile draws this
// full-viewport triangle (depth compare always, through the tile's viewport and scissor) first.

/**
 * One triangle covering the viewport, on the far plane.
 */
@vertex fn vertexMain(@builtin(vertex_index) vertexIndex: u32) -> @builtin(position) vec4f {
  let position = vec2f(f32((vertexIndex << 1u) & 2u), f32(vertexIndex & 2u));
  return vec4f((position*2.0) - 1.0, 1.0, 1.0);
}
