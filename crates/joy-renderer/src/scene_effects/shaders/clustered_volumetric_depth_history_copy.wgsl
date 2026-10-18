// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/clustered-volumetric-lighting.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

struct ClusteredVolumetricDepthHistoryCopyUniforms {
  inverseProjectionMatrix: mat4x4f,
};

@group(0) @binding(auto) var<uniform> clusteredVolumetricDepthHistoryCopy:
  ClusteredVolumetricDepthHistoryCopyUniforms;
@group(0) @binding(auto) var depthTexture: texture_depth_2d;
@group(0) @binding(auto) var depthTextureSampler: sampler;

fn clusteredVolumetricDepthHistoryCopy_sampleColor(
  sourceTexture: texture_2d<f32>,
  sourceTextureSampler: sampler,
  texSize: vec2f,
  texCoord: vec2f
) -> vec4f {
  let depth = textureSampleLevel(depthTexture, depthTextureSampler, texCoord, 0);
  let clip = vec4f(texCoord.x * 2.0 - 1.0, 1.0 - texCoord.y * 2.0, depth, 1.0);
  let viewPosition = clusteredVolumetricDepthHistoryCopy.inverseProjectionMatrix * clip;
  let linearDepth = abs(viewPosition.z / max(abs(viewPosition.w), 0.00001));
  return vec4f(linearDepth, 0.0, 0.0, 1.0);
}
