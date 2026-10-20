// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Unlit shading model: the surface shows its color unchanged, without lights or shadows. Implements the contract in
// surface-contract.wgsl.

const MATERIAL_RECEIVES_LIGHT: bool = false;

struct MaterialParameters {
  // X alpha cutoff, Y alpha mode (0 opaque, 1 mask, 2 blend), ZW reserved.
  coverage: vec4f,
  // Linear RGBA color; RGB may exceed one.
  baseColor: vec4f,
};

@group(3) @binding(0) var<uniform> material: MaterialParameters;
@group(3) @binding(1) var baseColorMap: texture_2d<f32>;
@group(3) @binding(2) var materialSampler: sampler;

struct MaterialSurface {
  normal: vec3f,
  color: vec4f,
};

fn materialSurface(input: SurfaceInput) -> MaterialSurface {
  let sampled = textureSample(baseColorMap, materialSampler, input.uv);
  var surface: MaterialSurface;
  surface.normal = input.faceNormal;
  surface.color = input.color*material.baseColor*sampled;
  return surface;
}

fn materialOpacity(input: SurfaceInput) -> f32 {
  return input.color.a*material.baseColor.a*textureSample(baseColorMap, materialSampler, input.uv).a;
}

fn materialAlpha(surface: MaterialSurface) -> f32 {
  return surface.color.a;
}

fn materialDirect(surface: MaterialSurface, light: LightSample) -> vec3f {
  return vec3f(0.0);
}

fn materialIndirect(surface: MaterialSurface, ambient: AmbientLight) -> vec3f {
  return vec3f(0.0);
}

fn materialEmission(surface: MaterialSurface) -> vec3f {
  return surface.color.rgb;
}

fn materialAttributes(surface: MaterialSurface) -> SurfaceAttributes {
  var attributes: SurfaceAttributes;
  attributes.normal = surface.normal;
  attributes.roughness = 1.0;
  attributes.diffuseAlbedo = vec3f(0.0);
  attributes.specularColor = vec3f(0.0);
  attributes.metallic = 0.0;
  attributes.emissive = surface.color.rgb;
  attributes.occlusion = 1.0;
  return attributes;
}
