// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

struct animatedGrainUniforms {
  intensity: f32,
  size: f32,
  speed: f32,
  noiseVariant: f32,
  timestamp: f32,
};

@group(0) @binding(auto) var<uniform> animatedGrain: animatedGrainUniforms;

/**
 * Filters a color with animated film grain effect.
 * @param color The input color.
 * @param texSize The size of the texture.
 * @param texCoord The texture coordinates.
 * @returns The filtered color.
 */
fn animatedGrain_filterColor_ext(
  color: vec4f,
  texSize: vec2f,
  texCoord: vec2f
  ) -> vec4f {
  let grainCell = floor((texCoord*texSize)/max(animatedGrain.size, 0.5));
  let grainFrame = floor(animatedGrain.timestamp * animatedGrain.speed);
  let analogNoise = rand(grainCell + vec2f(grainFrame*37.0, grainFrame * 17.0));
  let offsetNoise = rand(grainCell.yx + vec2f(grainFrame*11.0, grainFrame*29.0) + 19.19);
  let decorrelatedNoise = (analogNoise + offsetNoise)*0.5;
  let noiseValue = mix(analogNoise, decorrelatedNoise, animatedGrain.noiseVariant);
  let luminance = dot(color.rgb, vec3f(0.2126, 0.7152, 0.0722));
  let midtoneWeight = 1.0 - smoothstep(0.35, 1.0, abs((luminance*2.0) - 1.0));
  let grain = (noiseValue - 0.5)*animatedGrain.intensity*mix(0.6, 1.0, midtoneWeight);

  return(vec4f(clamp(color.rgb + grain, vec3f(0.0), vec3f(1.0)), color.a));
}
