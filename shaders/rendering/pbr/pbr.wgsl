// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// World-space GGX metallic/roughness shading; matches the GLSL path.
struct FrameUniforms {
  viewProjection: mat4x4<f32>,
  eye: vec4<f32>,
  lightDirection: vec4<f32>,
  lightColor: vec4<f32>,
  ambientColor: vec4<f32>
};

struct MaterialUniforms {
  baseColor: vec4<f32>,
  emissiveMetallic: vec4<f32>,
  roughnessAlpha: vec4<f32>
};

@group(0) @binding(0) var<uniform> frameUniforms: FrameUniforms;
@group(0) @binding(1) var<uniform> materialUniforms: MaterialUniforms;
@group(0) @binding(2) var baseColorTexture: texture_2d<f32>;
@group(0) @binding(3) var baseColorTextureSampler: sampler;
@group(0) @binding(4) var normalTexture: texture_2d<f32>;
@group(0) @binding(5) var normalTextureSampler: sampler;
@group(0) @binding(6) var emissiveTexture: texture_2d<f32>;
@group(0) @binding(7) var emissiveTextureSampler: sampler;
@group(0) @binding(8) var metallicTexture: texture_2d<f32>;
@group(0) @binding(9) var metallicTextureSampler: sampler;
@group(0) @binding(10) var roughnessTexture: texture_2d<f32>;
@group(0) @binding(11) var roughnessTextureSampler: sampler;
@group(0) @binding(12) var aoTexture: texture_2d<f32>;
@group(0) @binding(13) var aoTextureSampler: sampler;

struct VertexOutput {
  @builtin(position) position: vec4<f32>,
  @location(0) worldPosition: vec3<f32>,
  @location(1) worldNormal: vec3<f32>,
  @location(2) color: vec4<f32>,
  @location(3) uv: vec2<f32>
};

@vertex
fn vertexMain(
  @location(0) position: vec3<f32>,
  @location(1) normal: vec3<f32>,
  @location(2) color: vec4<f32>,
  @location(3) uv: vec2<f32>
) -> VertexOutput {
  var output: VertexOutput;
  output.position = frameUniforms.viewProjection*vec4<f32>(position, 1.0);
  output.worldPosition = position;
  output.worldNormal = normal;
  output.color = color;
  output.uv = uv;
  return output;
}

/**
  * PBR fragment shader that implements a GGX microfacet model with metallic/roughness workflow.
  * The shader is designed to match the GLSL path and produce identical results.
  */
@fragment fn fragmentMain(input: VertexOutput, @builtin(front_facing) frontFacing: bool) -> @location(0) vec4<f32> {
  // Sample before alpha discard so derivative-based tangent frames remain valid.
  let sampledBase = textureSample(baseColorTexture, baseColorTextureSampler, input.uv);
  let base = materialUniforms.baseColor.rgb*input.color.rgb*linearColor(sampledBase.rgb);
  var alpha = materialUniforms.baseColor.a*input.color.a*sampledBase.a;
  let emission = materialUniforms.emissiveMetallic.rgb*linearColor(textureSample(emissiveTexture, emissiveTextureSampler, input.uv).rgb);
  let metallic = clamp(materialUniforms.emissiveMetallic.a*textureSample(metallicTexture, metallicTextureSampler, input.uv).r, 0.0, 1.0);
  let roughness = clamp(materialUniforms.roughnessAlpha.x*textureSample(roughnessTexture, roughnessTextureSampler, input.uv).r, 0.045, 1.0);
  let occlusion = textureSample(aoTexture, aoTextureSampler, input.uv).r;
  let tangentNormal = textureSample(normalTexture, normalTextureSampler, input.uv).xyz*2.0 - 1.0;
  var n = normalize(input.worldNormal)*select(-1.0, 1.0, frontFacing);
  let dpX = dpdx(input.worldPosition);
  let dpY = dpdy(input.worldPosition);
  let uvX = dpdx(input.uv);
  let uvY = dpdy(input.uv);
  let determinant = uvX.x*uvY.y - uvX.y*uvY.x;

  var tangent = dpX*uvY.y - dpY*uvX.y;
  tangent-= n*dot(n, tangent);

  if(materialUniforms.roughnessAlpha.w > 0.5 && abs(determinant) > 0.0000001 && dot(tangent, tangent) > 0.0000001) {
    tangent = normalize(tangent)*sign(determinant);
    let bitangent = normalize(cross(n, tangent))*sign(determinant);
    n = normalize(tangent*tangentNormal.x + bitangent*tangentNormal.y + n*tangentNormal.z);
  }

  if(materialUniforms.roughnessAlpha.y < 0.5) {
    alpha = 1.0;
  } else if(materialUniforms.roughnessAlpha.y < 1.5) {
    if(alpha < materialUniforms.roughnessAlpha.z) {
      discard;
    }
    alpha = 1.0;
  }

  let v = normalize(frameUniforms.eye.xyz - input.worldPosition);
  let l = normalize(frameUniforms.lightDirection.xyz);
  let h = normalize(v + l);
  let nv = max(dot(n, v), 0.0001);
  let nl = max(dot(n, l), 0.0);
  let nh = max(dot(n, h), 0.0);
  let vh = max(dot(v, h), 0.0);
  let a2 = pow(roughness, 4.0);
  let denominator = nh*nh*(a2 - 1.0) + 1.0;
  let distribution = a2/max(PI*denominator*denominator, 0.000000000001);
  let visibility = 0.5/max(nl*sqrt(nv*nv*(1.0 - a2) + a2) + nv*sqrt(nl*nl*(1.0 - a2) + a2), 0.0001);
  let f0 = mix(vec3<f32>(0.04), base, metallic);
  let fresnel = f0 + (1.0 - f0)*pow(1.0 - vh, 5.0);
  let diffuse = (1.0 - fresnel)*(1.0 - metallic)*base/PI;

  var radiance = (diffuse + distribution*visibility*fresnel)*frameUniforms.lightColor.rgb*nl;
  radiance+= frameUniforms.ambientColor.rgb*base*(1.0 - metallic)*occlusion + emission;

  if(frameUniforms.ambientColor.a > 0.5) {
    radiance = displayColor(radiance);
  }

  return(vec4<f32>(radiance, alpha));
}
