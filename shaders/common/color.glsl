// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Converts an sRGB color to linear light before lighting or compositing operations.
 * The piecewise transfer function preserves the standard near-black segment instead of approximating the entire curve with a power function.
 */
vec3 linearColor(vec3 color) {
  return(mix(color/12.92, pow((color + 0.055)/1.055, vec3(2.4)), step(vec3(0.04045), color)));
}

/**
 * Compresses a linear HDR color and encodes it for an sRGB display.
 * Reinhard compression is intentionally applied before the piecewise transfer function so out-of-range lighting remains finite.
 */
vec3 displayColor(vec3 color) {
  vec3 mapped = color/(1.0 + color);

  return(mix(mapped*12.92, 1.055*pow(mapped, vec3(1.0/2.4)) - 0.055, step(vec3(0.0031308), mapped)));
}
