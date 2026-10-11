// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

struct CasterCamera {viewProjection: mat4x4f};
struct MaterialUniforms {baseColor: vec4f, emissiveMetallic: vec4f, roughnessAlpha: vec4f};
@group(0) @binding(0) var<uniform> casterCamera: CasterCamera;
@group(0) @binding(1) var<uniform> materialUniforms: MaterialUniforms;
@group(0) @binding(2) var baseColorTexture: texture_2d<f32>;
@group(0) @binding(3) var baseColorTextureSampler: sampler;
struct CasterOutput {
  @builtin(position) position: vec4f,
  @location(0) uv: vec2f,
  @location(1) alpha: f32
};
@vertex fn vertexMain(@location(0) position: vec3f, @location(2) color: vec4f, @location(3) uv: vec2f) -> CasterOutput {
  var output: CasterOutput;
  output.position = casterCamera.viewProjection*vec4f(position, 1);
  output.uv = uv;
  output.alpha = color.a;
  return output;
}
@fragment fn fragmentMain(input: CasterOutput) {
  let alpha = textureSample(baseColorTexture, baseColorTextureSampler, input.uv).a*materialUniforms.baseColor.a*input.alpha;
  if(materialUniforms.roughnessAlpha.y > 0.5 && alpha < materialUniforms.roughnessAlpha.z) {
    discard;
  }
}
