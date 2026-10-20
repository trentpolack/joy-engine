// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Lit shading model: the engine's flat, faceted, double-sided stylized lighting. Faces light by their rasterized
// (derivative) normals, so meshes read as faceted however their normals were authored, and both sides respond to
// light (|n.l|). Diffuse terms omit the 1/pi factor so authored colors keep their brightness under unit light.
// Implements the contract in surface-contract.wgsl.

const MATERIAL_RECEIVES_LIGHT: bool = true;

struct MaterialParameters {
  // X alpha cutoff, Y alpha mode (0 opaque, 1 mask, 2 blend), ZW reserved.
  coverage: vec4f,
  // Linear RGBA base color.
  baseColor: vec4f,
  // Linear RGB emission; A reserved.
  emissive: vec4f,
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
  return surface.color.rgb*light.radiance*abs(dot(surface.normal, light.direction));
}

fn materialIndirect(surface: MaterialSurface, ambient: AmbientLight) -> vec3f {
  return surface.color.rgb*ambientIrradiance(ambient, surface.normal);
}

fn materialEmission(surface: MaterialSurface) -> vec3f {
  return material.emissive.rgb;
}

fn materialAttributes(surface: MaterialSurface) -> SurfaceAttributes {
  var attributes: SurfaceAttributes;
  attributes.normal = surface.normal;
  attributes.roughness = 1.0;
  attributes.diffuseAlbedo = surface.color.rgb;
  attributes.specularColor = vec3f(0.0);
  attributes.metallic = 0.0;
  attributes.emissive = material.emissive.rgb;
  attributes.occlusion = 1.0;
  return attributes;
}

fn materialEncode(surface: MaterialSurface) -> vec3u {
  return vec3u(packRgb9e5(surface.color.rgb), 0u, 0u);
}

fn materialDecode(payload: vec3u, normal: vec3f, view: vec3f) -> MaterialSurface {
  var surface: MaterialSurface;
  surface.normal = normal;
  surface.color = vec4f(unpackRgb9e5(payload.x), 1.0);
  return surface;
}
