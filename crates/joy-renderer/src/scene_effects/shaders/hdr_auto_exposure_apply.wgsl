// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/hdr-auto-exposure.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

struct HDRAutoExposureApplyUniforms {
  debugMode: f32,
  enabled: f32,
};

@group(0) @binding(auto) var<uniform> hdrAutoExposureApply: HDRAutoExposureApplyUniforms;
@group(0) @binding(auto) var exposureTexture: texture_2d<f32>;
@group(0) @binding(auto) var exposureTextureSampler: sampler;

fn hdrAutoExposureApply_sampleColor(
  sourceTexture: texture_2d<f32>,
  sourceTextureSampler: sampler,
  texSize: vec2f,
  texCoord: vec2f
) -> vec4f {
  let source = textureSampleLevel(sourceTexture, sourceTextureSampler, texCoord, 0);
  let exposureState = textureSampleLevel(exposureTexture, exposureTextureSampler, vec2f(0.5), 0);
  if (hdrAutoExposureApply.debugMode > 0.5) {
    let sceneLuminance = max(dot(source.rgb, vec3f(0.2126, 0.7152, 0.0722)), 0.0001);
    let normalizedLuminance = clamp((log2(sceneLuminance) + 5.0) / 10.0, 0.0, 1.0);
    let shadows = vec3f(0.06, 0.10, 0.40);
    let midtones = vec3f(0.15, 0.82, 0.58);
    let highlights = vec3f(1.0, 0.63, 0.08);
    let heatMap = mix(
      mix(shadows, midtones, smoothstep(0.0, 0.5, normalizedLuminance)),
      highlights,
      smoothstep(0.5, 1.0, normalizedLuminance)
    );
    return vec4f(heatMap, source.a);
  }

  if (hdrAutoExposureApply.enabled < 0.5) {
    return source;
  }

  return vec4f(source.rgb * exposureState.r, source.a);
}
