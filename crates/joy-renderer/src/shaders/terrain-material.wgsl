// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Terrain shading model: slope and altitude coloring under the engine's stylized lighting, with the LOD diagnostics.
// Implements the contract in surface-contract.wgsl against the material group terrain-geometry.wgsl declares, and
// matches the lit model's lighting response and G-buffer encoding exactly, so deferred terrain pixels shade in the lit
// model's lighting pass. Unlike the lit model it shades with the smooth normal of the morphed surface rather than the
// faceted triangle normal, since terrain triangles are a level of detail rather than authored facets.
//
// The terrain vertex stage fills the inputs: `vertexNormal` with the smooth surface normal, `color` with the LOD tint of
// the diagnostic modes (white otherwise), and `uv` with lattice cell coordinates at the drawn stride.

const MATERIAL_RECEIVES_LIGHT: bool = true;

// Colors of the slope and altitude rules (linear).
const TERRAIN_GROUND_COLOR: vec3f = vec3f(0.21, 0.36, 0.26);
const TERRAIN_ROCK_COLOR: vec3f = vec3f(0.49, 0.43, 0.36);
const TERRAIN_SNOW_COLOR: vec3f = vec3f(0.79, 0.82, 0.77);

// Diagnostic modes (`TerrainDiagnostic` in Rust).
const TERRAIN_LOD_COLORS: f32 = 1.0;
const TERRAIN_GRID_OVERLAY: f32 = 2.0;

struct MaterialSurface {
  normal: vec3f,
  color: vec4f,
};

fn materialSurface(input: SurfaceInput) -> MaterialSurface {
  var surface: MaterialSurface;
  surface.normal = surfaceNormal(input);
  surface.color = vec4f(terrainAlbedo(input, surface.normal), 1.0);
  return surface;
}

fn materialOpacity(input: SurfaceInput) -> f32 {
  return 1.0;
}

fn materialAlpha(surface: MaterialSurface) -> f32 {
  return 1.0;
}

fn materialDirect(surface: MaterialSurface, light: LightSample) -> vec3f {
  return surface.color.rgb*light.radiance*abs(dot(surface.normal, light.direction));
}

fn materialIndirect(surface: MaterialSurface, ambient: AmbientLight) -> vec3f {
  return surface.color.rgb*ambientIrradiance(ambient, surface.normal);
}

fn materialEmission(surface: MaterialSurface) -> vec3f {
  return vec3f(0.0);
}

fn materialAttributes(surface: MaterialSurface) -> SurfaceAttributes {
  var attributes: SurfaceAttributes;
  attributes.normal = surface.normal;
  attributes.roughness = 1.0;
  attributes.diffuseAlbedo = surface.color.rgb;
  attributes.specularColor = vec3f(0.0);
  attributes.metallic = 0.0;
  attributes.emissive = vec3f(0.0);
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

/**
 * Slope and altitude coloring, or the LOD diagnostics. The material rules match the CPU's detail synthesis, so rough
 * detail appears where rock is drawn. Grid lines come from the lattice coordinates' screen derivatives, which the
 * surface input took in uniform control flow.
 */
fn terrainAlbedo(input: SurfaceInput, normal: vec3f) -> vec3f {
  let slope = 1.0 - abs(normal.y);
  let rock = smoothstep(material.materials.x, material.materials.y, slope);
  let snow = smoothstep(material.materials.z, material.materials.w, input.worldPosition.y);
  var color = mix(mix(TERRAIN_GROUND_COLOR, TERRAIN_ROCK_COLOR, rock), TERRAIN_SNOW_COLOR, snow);
  if(material.geometry.y >= TERRAIN_LOD_COLORS) {
    color = input.color.rgb;
  }

  // The grid overlay darkens lines about one pixel wide over the LOD colors.
  if(material.geometry.y >= TERRAIN_GRID_OVERLAY) {
    let width = max(abs(input.uvDx) + abs(input.uvDy), vec2f(0.0001));
    let lines = abs(fract(input.uv - 0.5) - 0.5)/width;
    color*= mix(0.3, 1.0, smoothstep(0.0, 1.0, min(lines.x, lines.y)));
  }
  return color;
}
