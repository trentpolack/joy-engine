// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Alpha-tested shadow caster for submeshes with masked materials: texels below the cutoff leave holes in the depth map,
// so masked surfaces cast the coverage the camera sees.

struct Caster {
  lightViewProjection: mat4x4f,
};

@group(0) @binding(0) var<uniform> caster: Caster;
@group(1) @binding(1) var baseColorMap: texture_2d<f32>;
@group(1) @binding(7) var materialSampler: sampler;

struct CasterOutput {
  @builtin(position) position: vec4f,
  @location(0) uv: vec2f,
  @location(1) alpha: f32,
  // Alpha cutoff (X) and alpha mode (Y).
  @location(2) alphaTest: vec2f,
};

@vertex fn vertexMain(
  @location(0) position: vec3f,
  @location(2) color: vec4f,
  @location(3) surface: vec4f,
  @location(5) model0: vec4f,
  @location(6) model1: vec4f,
  @location(7) model2: vec4f,
  @location(8) model3: vec4f,
  @location(15) uv: vec2f
) -> CasterOutput {
  let model = mat4x4f(model0, model1, model2, model3);
  var output: CasterOutput;
  output.position = caster.lightViewProjection*(model*vec4f(position, 1.0));
  output.uv = uv;
  output.alpha = color.a;
  output.alphaTest = surface.zw;
  return output;
}

@fragment fn fragmentMain(input: CasterOutput) {
  let alpha = textureSample(baseColorMap, materialSampler, input.uv).a*input.alpha;
  if((input.alphaTest.y > 0.5) && (input.alphaTest.y < 1.5) && (alpha < input.alphaTest.x)) {
    discard;
  }
}
