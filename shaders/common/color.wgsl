// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Convert sRGB input to linear RGB for lighting calculations.
 * @param color The sRGB color to convert.
 * @returns The linear RGB color.
 */
fn linearColor(color: vec3<f32>) -> vec3<f32> {
  return(select(color/12.92, pow((color + 0.055)/1.055, vec3<f32>(2.4)), color >= vec3<f32>(0.04045)));
}

/** 
 * Convert the shader's linear HDR color to a displayable sRGB color.
 * This is a simple tonemapping operator (Reinhard) that compresses the HDR range into the displayable range.
 * @param color The linear RGB color to convert.
 * @returns The sRGB color.
 */
fn displayColor(color: vec3<f32>) -> vec3<f32> {
  let mapped = color/(1.0 + color);
  return(select(mapped*12.92, 1.055*pow(mapped, vec3<f32>(1.0/2.4)) - 0.055, mapped >= vec3<f32>(0.0031308)));
}
