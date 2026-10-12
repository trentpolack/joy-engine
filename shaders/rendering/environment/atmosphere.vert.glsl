// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

precision highp float;

out vec2 screenPosition;

/**
 * Builds a full-screen triangle without allocating a vertex buffer.
 * The oversized triangle covers the viewport while avoiding a diagonal seam between two screen-space primitives.
 */
void main() {
  vec2 position = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
  screenPosition = position*2.0 - 1.0;

  gl_Position = vec4(screenPosition, 0.0, 1.0);
}

