// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

struct vignetteBlurUniforms {
  amount: f32,
  radius: f32,
};

@group(0) @binding(auto) var<uniform> vignetteBlur: vignetteBlurUniforms;
@group(0) @binding(auto) var blurredTexture: texture_2d<f32>;
@group(0) @binding(auto) var blurredTextureSampler: sampler;

/**
 * Blends a pre-blurred image into the source near the viewport boundary.
 * Aspect-corrected distance keeps the vignette circular on non-square targets, and source alpha remains authoritative for compositing.
 *  TODO (trent, 10/12/26): Implement proper vignette blur logic and not a texture sampled one.
 */
fn vignetteBlur_sampleColor(
  sourceTexture: texture_2d<f32>,
  sourceTextureSampler: sampler,
  texSize: vec2f,
  texCoord: vec2f
) -> vec4f {
  let aspectScale = texSize/max(min(texSize.x, texSize.y), 1.0);
  let edgeDistance = length((texCoord - vec2f(0.5))*aspectScale);
  let edgeStart = vignetteBlur.radius;
  let edgeEnd = max(0.82, edgeStart + 0.01);
  let edgeWeight = smoothstep(edgeStart, edgeEnd, edgeDistance)*vignetteBlur.amount;
  let sourceColor = textureSample(sourceTexture, sourceTextureSampler, texCoord);
  let blurredColor = textureSample(blurredTexture, blurredTextureSampler, texCoord).rgb;

  return(vec4f(mix(sourceColor.rgb, blurredColor, edgeWeight), sourceColor.a));
}
