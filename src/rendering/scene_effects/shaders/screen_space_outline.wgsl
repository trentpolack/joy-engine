// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/outlines.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

struct screenSpaceOutlineUniforms {
  color: vec4f,
  thickness: f32,
  depthThreshold: f32,
  normalThreshold: f32,
  useNormalTexture: f32,
};
@group(0) @binding(auto) var<uniform> screenSpaceOutline: screenSpaceOutlineUniforms;
@group(0) @binding(auto) var depthTexture: texture_depth_2d;
@group(0) @binding(auto) var depthTextureSampler: sampler;
@group(0) @binding(auto) var normalTexture: texture_2d<f32>;
@group(0) @binding(auto) var normalTextureSampler: sampler;
fn advancedSceneUV(uv: vec2f) -> vec2f {
  return uv;
}

fn advancedLinearDepth(depth: f32, nearPlane: f32, farPlane: f32) -> f32 {
  return (nearPlane * farPlane) / max(farPlane - depth * (farPlane - nearPlane), 0.0001);
}

fn advancedDepthNormal(depthTexture: texture_depth_2d, depthTextureSampler: sampler, uv: vec2f) -> vec3f {
  let dimensions = vec2i(textureDimensions(depthTexture));
  let coordinate = clamp(vec2i(uv * vec2f(dimensions)), vec2i(0), dimensions - vec2i(1));
  let center = textureLoad(depthTexture, coordinate, 0);
  let right = textureLoad(depthTexture, min(coordinate + vec2i(1, 0), dimensions - vec2i(1)), 0);
  let up = textureLoad(depthTexture, min(coordinate + vec2i(0, 1), dimensions - vec2i(1)), 0);
  return normalize(vec3f((center - right) * f32(dimensions.x), (center - up) * f32(dimensions.y), 1.0));
}

fn screenSpaceOutline_sampleColor(
  sourceTexture: texture_2d<f32>, sourceTextureSampler: sampler, texSize: vec2f, texCoord: vec2f
) -> vec4f {
  let texel = screenSpaceOutline.thickness / vec2f(textureDimensions(depthTexture));
  let centerSceneUv = advancedSceneUV(texCoord);
  let centerDepth = textureSample(depthTexture, depthTextureSampler, centerSceneUv);
  let centerReconstructed = advancedDepthNormal(depthTexture, depthTextureSampler, centerSceneUv);
  let centerTextureNormal = normalize(textureSample(normalTexture, normalTextureSampler, centerSceneUv).xyz * 2.0 - 1.0);
  let centerNormal = normalize(mix(centerReconstructed, centerTextureNormal, screenSpaceOutline.useNormalTexture));
  var depthEdge = 0.0;
  var normalEdge = 0.0;
  let offsets = array<vec2f, 4>(vec2f(texel.x, 0.0), vec2f(-texel.x, 0.0), vec2f(0.0, texel.y), vec2f(0.0, -texel.y));
  for (var index: i32 = 0; index < 4; index++) {
    let sampleUv = clamp(texCoord + offsets[index], vec2f(0.0), vec2f(1.0));
    let sampleSceneUv = advancedSceneUV(sampleUv);
    let sampleDepth = textureSample(depthTexture, depthTextureSampler, sampleSceneUv);
    depthEdge = max(depthEdge, abs(sampleDepth - centerDepth));
    let reconstructed = advancedDepthNormal(depthTexture, depthTextureSampler, sampleSceneUv);
    let textureNormal = normalize(textureSample(normalTexture, normalTextureSampler, sampleSceneUv).xyz * 2.0 - 1.0);
    let sampleNormal = normalize(mix(reconstructed, textureNormal, screenSpaceOutline.useNormalTexture));
    normalEdge = max(normalEdge, 1.0 - max(dot(centerNormal, sampleNormal), 0.0));
  }
  let edge = max(smoothstep(screenSpaceOutline.depthThreshold, screenSpaceOutline.depthThreshold * 2.0, depthEdge),
                 smoothstep(screenSpaceOutline.normalThreshold, screenSpaceOutline.normalThreshold * 2.0, normalEdge));
  let sourceColor = textureSample(sourceTexture, sourceTextureSampler, texCoord);
  return mix(sourceColor, screenSpaceOutline.color, edge * screenSpaceOutline.color.a);
}
