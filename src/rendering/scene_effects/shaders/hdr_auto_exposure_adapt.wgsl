// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/hdr-auto-exposure.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

struct HDRAutoExposureAdaptUniforms {
  keyValue: f32,
  minimumExposure: f32,
  maximumExposure: f32,
  brightenSpeed: f32,
  darkenSpeed: f32,
  deltaTime: f32,
  enabled: f32,
};

@group(0) @binding(auto) var<uniform> hdrAutoExposureAdapt: HDRAutoExposureAdaptUniforms;
@group(0) @binding(auto) var historyTexture: texture_2d<f32>;
@group(0) @binding(auto) var historyTextureSampler: sampler;

fn hdrAutoExposureAdapt_sampleColor(
  sourceTexture: texture_2d<f32>,
  sourceTextureSampler: sampler,
  texSize: vec2f,
  texCoord: vec2f
) -> vec4f {
  if (hdrAutoExposureAdapt.enabled < 0.5) {
    return vec4f(1.0, 0.0, 0.0, 1.0);
  }

  let sourceDimensions = textureDimensions(sourceTexture);
  var meterState = vec2f(0.0);
  for (var sampleY: u32 = 0u; sampleY < sourceDimensions.y; sampleY++) {
    for (var sampleX: u32 = 0u; sampleX < sourceDimensions.x; sampleX++) {
      meterState += textureLoad(sourceTexture, vec2i(i32(sampleX), i32(sampleY)), 0).rg;
    }
  }

  let logLuminance = meterState.x / max(meterState.y, 0.0001);
  let averageLuminance = max(exp(logLuminance), 0.0001);
  let minimumExposure = max(
    min(hdrAutoExposureAdapt.minimumExposure, hdrAutoExposureAdapt.maximumExposure),
    0.0001
  );
  let maximumExposure = max(
    max(hdrAutoExposureAdapt.minimumExposure, hdrAutoExposureAdapt.maximumExposure),
    minimumExposure
  );
  let targetExposure = clamp(
    hdrAutoExposureAdapt.keyValue / averageLuminance,
    minimumExposure,
    maximumExposure
  );
  let previousExposure = clamp(
    textureSampleLevel(historyTexture, historyTextureSampler, vec2f(0.5), 0).r,
    minimumExposure,
    maximumExposure
  );
  let adaptationSpeed = select(
    hdrAutoExposureAdapt.darkenSpeed,
    hdrAutoExposureAdapt.brightenSpeed,
    targetExposure > previousExposure
  );
  let adaptationWeight = 1.0 - exp(-adaptationSpeed * max(hdrAutoExposureAdapt.deltaTime, 0.0));
  let exposure = clamp(
    mix(previousExposure, targetExposure, adaptationWeight),
    minimumExposure,
    maximumExposure
  );
  return vec4f(exposure, averageLuminance, targetExposure, 1.0);
}
