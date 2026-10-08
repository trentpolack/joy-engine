// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Convert sRGB input to linear RGB for lighting calculations.
 * @param color The sRGB color to convert.
 * @returns The linear RGB color.
 */
vec3 linearColor(vec3 color) {
  return(mix(color/12.92, pow((color + 0.055)/1.055, vec3(2.4)), step(vec3(0.04045), color)));
}

/** 
 * Convert the shader's linear HDR color to a displayable sRGB color.
 * This is a simple tonemapping operator (Reinhard) that compresses the HDR range into the displayable range.
 * @param color The linear RGB color to convert.
 * @returns The sRGB color.
 */
vec3 displayColor(vec3 color) {
  vec3 mapped = color/(1.0 + color);
  return(mix(mapped*12.92, 1.055*pow(mapped, vec3(1.0/2.4)) - 0.055, step(vec3(0.0031308), mapped)));
}
