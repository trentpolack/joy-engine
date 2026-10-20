// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// The shading-model contract: the records the engine's surface passes hand to a shading model and the records a model
// returns. A model file (`models/*.wgsl`, or a game's own) declares its material group and implements:
//
//   const MATERIAL_RECEIVES_LIGHT: bool;                       // false skips the light loop and shadows (unlit)
//   struct MaterialParameters { coverage: vec4f, ... };        // group 3, binding 0; `coverage` comes first
//   @group(3) @binding(0) var<uniform> material: MaterialParameters;
//   struct MaterialSurface { ... };                            // whatever the model needs per fragment
//   fn materialSurface(input: SurfaceInput) -> MaterialSurface;
//   fn materialOpacity(input: SurfaceInput) -> f32;            // alpha alone, for depth-only passes
//   fn materialAlpha(surface: MaterialSurface) -> f32;
//   fn materialDirect(surface: MaterialSurface, light: LightSample) -> vec3f;
//   fn materialIndirect(surface: MaterialSurface, ambient: AmbientLight) -> vec3f;
//   fn materialEmission(surface: MaterialSurface) -> vec3f;
//   fn materialAttributes(surface: MaterialSurface) -> SurfaceAttributes;
//
// A model that also implements the two functions below supports deferred lighting. Its opaque surfaces then write
// emission and indirect light plus a G-buffer texel in one pass, and the engine adds direct light in one full-screen
// pass per model, so a pixel pays for the lights that reach it once instead of once per overlapping surface:
//
//   fn materialEncode(surface: MaterialSurface) -> vec3u;      // 96 bits for the G-buffer (gbuffer-encoding.wgsl)
//   fn materialDecode(payload: vec3u, normal: vec3f, view: vec3f) -> MaterialSurface;
//
// `materialDecode` receives the shading normal from `materialAttributes` and the unit direction toward the eye, and
// only `materialDirect` reads its result. Both run where the material group is not bound, so neither may read it.
// Models without them shade forward, as do all surfaces on WebGL 2.
//
// The engine owns everything else: vertex transforms, the light loop, light clusters, directional and point shadows,
// the skylight, image-based lighting, alpha testing, motion vectors, scene-effect attachments, the G-buffer layout, and
// debug views. Model functions read only their inputs, their material group, and the environment helpers below; they
// never read the frame or shadow groups, so the same model file also compiles into the depth pre-pass and shadow
// casters.
//
// Texture sampling with implicit derivatives (`textureSample`) must happen in `materialSurface` and `materialOpacity`,
// which the engine calls in uniform control flow before any alpha test discards a fragment.

// The material's coverage header, which every `MaterialParameters` struct starts with: X alpha cutoff, Y alpha mode
// (0 opaque, 1 mask, 2 blend), ZW reserved.
const ALPHA_MODE_MASK: f32 = 1.0;
const ALPHA_MODE_BLEND: f32 = 2.0;

// Everything a model can know about one fragment.
struct SurfaceInput {
  // World-space position.
  worldPosition: vec3f,
  // Interpolated world-space vertex normal, flipped for back faces; zero where the mesh authored none.
  vertexNormal: vec3f,
  // The rasterized triangle's normal from screen-space derivatives, oriented toward the eye.
  faceNormal: vec3f,
  // Unit direction from the surface toward the eye.
  viewDirection: vec3f,
  // Texture coordinates (top-left origin) and their screen-space derivatives, for tangent frames.
  uv: vec2f,
  uvDx: vec2f,
  uvDy: vec2f,
  // Screen-space derivatives of the world position.
  positionDx: vec3f,
  positionDy: vec3f,
  // Vertex color times the instance tint, straight alpha (instance opacity included).
  color: vec4f,
  // Whether the fragment is on the triangle's front face.
  frontFacing: bool,
};

// One light reaching a fragment.
struct LightSample {
  // Unit direction from the surface toward the light.
  direction: vec3f,
  // Linear radiance arriving along it: color, intensity, distance attenuation, and shadow visibility applied.
  radiance: vec3f,
};

// Light arriving from every direction at once.
struct AmbientLight {
  // Uniform irradiance from every direction.
  uniformIrradiance: vec3f,
  // Skylight radiance from overhead and reflected up from the ground, intensity applied.
  sky: vec3f,
  ground: vec3f,
  // Image-based lighting: X intensity (zero without an environment map), Y horizontal rotation in radians, Z the
  // specular map's highest mip.
  environment: vec3f,
};

// The surface description every model reports, whatever its shading: scene effects, debug views, and shadow receivers
// read it. Colors are linear.
struct SurfaceAttributes {
  // Unit world-space shading normal, oriented toward the eye.
  normal: vec3f,
  // Perceptual roughness in [0, 1]; one for surfaces without a specular response.
  roughness: f32,
  // Diffuse albedo.
  diffuseAlbedo: vec3f,
  // Specular reflectance at normal incidence.
  specularColor: vec3f,
  // Conductor amount in [0, 1].
  metallic: f32,
  // Emitted radiance.
  emissive: vec3f,
  // Ambient occlusion in [0, 1].
  occlusion: f32,
};

/**
 * Irradiance from the ambient terms on a surface facing `normal`: the uniform term plus the sky blended toward the
 * ground's reflected light as the normal tilts down (the exact irradiance of two uniform hemispheres).
 */
fn ambientIrradiance(ambient: AmbientLight, normal: vec3f) -> vec3f {
  let skyWeight = clamp(0.5 + (0.5*normal.y), 0.0, 1.0);
  return ambient.uniformIrradiance + mix(ambient.ground, ambient.sky, skyWeight);
}

/**
 * A unit normal for `input`: the interpolated vertex normal when the mesh authored one, else the faceted triangle
 * normal. Double-sided surfaces keep the authored normal on front faces even where smooth shading tips it away from
 * the eye, so silhouettes keep their grazing-angle response.
 */
fn surfaceNormal(input: SurfaceInput) -> vec3f {
  if(dot(input.vertexNormal, input.vertexNormal) < 0.000001) {
    return input.faceNormal;
  }
  return normalize(input.vertexNormal);
}

/**
 * Perturb `normal` by a tangent-space normal-map sample (already decoded to [-1, 1]) through a tangent frame built from
 * screen-space derivatives. Where the texture coordinates are degenerate the normal is returned unchanged.
 */
fn perturbNormal(input: SurfaceInput, normal: vec3f, tangentNormal: vec3f) -> vec3f {
  let determinant = (input.uvDx.x*input.uvDy.y) - (input.uvDx.y*input.uvDy.x);
  var tangent = (input.positionDx*input.uvDy.y) - (input.positionDy*input.uvDx.y);
  tangent-= normal*dot(normal, tangent);

  // Degeneracy thresholds scale with the derivatives: screen-space derivatives at ordinary viewing distances fall below
  // fixed thresholds such as 1e-7, which would keep normal maps from applying.
  let uvScale = max(dot(input.uvDx, input.uvDx), dot(input.uvDy, input.uvDy));
  let positionScale = max(dot(input.positionDx, input.positionDx), dot(input.positionDy, input.positionDy));
  let mapped = (abs(determinant) > (0.000001*uvScale)) && (dot(tangent, tangent) > (0.000001*uvScale*positionScale));
  if(!mapped) {
    return normal;
  }
  let unitTangent = normalize(tangent)*sign(determinant);
  let bitangent = normalize(cross(normal, unitTangent))*sign(determinant);
  return normalize((unitTangent*tangentNormal.x) + (bitangent*tangentNormal.y) + (normal*tangentNormal.z));
}
