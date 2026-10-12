// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Converts an sRGB color to linear light before lighting or compositing operations.
 * The piecewise transfer function preserves the standard near-black segment instead of approximating the entire curve with a power function.
 */
fn linearColor(color: vec3<f32>) -> vec3<f32> {
  return(select(color/12.92, pow((color + 0.055)/1.055, vec3<f32>(2.4)), color >= vec3<f32>(0.04045)));
}

/**
 * Compresses a linear HDR color and encodes it for an sRGB display.
 * Reinhard compression is intentionally applied before the piecewise transfer function so out-of-range lighting remains finite.
 */
fn displayColor(color: vec3<f32>) -> vec3<f32> {
  let mapped = color/(1.0 + color);

  return(select(mapped*12.92, 1.055*pow(mapped, vec3<f32>(1.0/2.4)) - 0.055, mapped >= vec3<f32>(0.0031308)));
}
