// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/camera-reprojection-temporal-antialiasing.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

@group(0) @binding(auto) var depthTexture: texture_depth_2d;
@group(0) @binding(auto) var depthTextureSampler: sampler;

fn cameraReprojectionTaaDepthHistoryCopy_sampleColor(
  sourceTexture: texture_2d<f32>,
  sourceTextureSampler: sampler,
  texSize: vec2f,
  texCoord: vec2f
) -> vec4f {
  let depth = textureSampleLevel(depthTexture, depthTextureSampler, texCoord, 0);
  return vec4f(depth, 0.0, 0.0, 1.0);
}
