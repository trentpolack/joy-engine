// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/hdr-auto-exposure.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

fn hdrLuminanceExtract_sampleColor(
  sourceTexture: texture_2d<f32>,
  sourceTextureSampler: sampler,
  texSize: vec2f,
  texCoord: vec2f
) -> vec4f {
  let sourceTexel = 1.0 / vec2f(textureDimensions(sourceTexture));
  var weightedLogLuminance = 0.0;
  var totalWeight = 0.0;

  for (var sampleY: i32 = 0; sampleY < 4; sampleY++) {
    for (var sampleX: i32 = 0; sampleX < 4; sampleX++) {
      let sampleOffset = vec2f(f32(sampleX) - 1.5, f32(sampleY) - 1.5);
      let sampleCoord = clamp(texCoord + sampleOffset * sourceTexel, vec2f(0.0), vec2f(1.0));
      let sourceColor = textureSampleLevel(sourceTexture, sourceTextureSampler, sampleCoord, 0).rgb;
      let luminance = max(dot(max(sourceColor, vec3f(0.0)), vec3f(0.2126, 0.7152, 0.0722)), 0.0001);
      let distanceFromCenter = distance(sampleCoord, vec2f(0.5));
      let centerWeight = mix(1.0, 0.35, smoothstep(0.18, 0.72, distanceFromCenter));
      weightedLogLuminance += log(luminance) * centerWeight;
      totalWeight += centerWeight;
    }
  }

  return vec4f(weightedLogLuminance, totalWeight, 0.0, 1.0);
}
