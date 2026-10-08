// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

layout(std140) uniform animatedGrainUniforms {
  float intensity;
  float size;
  float speed;
  float noiseVariant;
  float timestamp;
} animatedGrain;

/**
 * Filters a color with animated film grain effect.
 * @param color The input color.
 * @param texSize The size of the texture.
 * @param texCoord The texture coordinates.
 * @returns The filtered color.
 */
vec4 animatedGrain_filterColor_ext(vec4 color, vec2 texSize, vec2 texCoord) {
  vec2 grainCell = floor((texCoord*texSize)/max(animatedGrain.size, 0.5));
  float grainFrame = floor(animatedGrain.timestamp * animatedGrain.speed);
  float analogNoise = rand(grainCell + vec2(grainFrame*37.0, grainFrame * 17.0));
  float offsetNoise = rand(grainCell.yx + vec2(grainFrame*11.0, grainFrame*29.0) + 19.19);
  float decorrelatedNoise = (analogNoise + offsetNoise)*0.5;
  float noiseValue = mix(analogNoise, decorrelatedNoise, animatedGrain.noiseVariant);
  float luminance = dot(color.rgb, vec3(0.2126, 0.7152, 0.0722));
  float midtoneWeight = 1.0 - smoothstep(0.35, 1.0, abs((luminance*2.0) - 1.0));
  float grain = (noiseValue - 0.5)*animatedGrain.intensity*mix(0.6, 1.0, midtoneWeight);

  return(vec4(clamp(color.rgb + grain, 0.0, 1.0), color.a));
}
