// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Mesh scene pass: the entry points every shading model is drawn with in forward shading. Composed after
// scene-common.wgsl, shadow-receiver.wgsl, scene-vertex.wgsl, surface-contract.wgsl, gbuffer-encoding.wgsl,
// environment-lighting.wgsl, one model file, and surface-lighting.wgsl.
//
// The vertex stage transforms instanced mesh vertices; the fragment stage builds the model's `SurfaceInput`, asks the
// model for its surface, applies the material's alpha test, and runs the engine's light loop (the primary directional
// light with cascaded shadows, the cluster's point lights with their shadows, ambient and skylight, and image-based
// light) through the model's response functions. Debug views replace the lit color with one of the surface attributes.
// Deferred shading reuses the vertex stage and helpers here with the G-buffer entry points in surface-gbuffer.wgsl.

struct SurfaceVarying {
  // Invariant so the depth pre-pass and every scene pipeline rasterize identical depth.
  @builtin(position) @invariant position: vec4f,
  @location(0) worldPosition: vec3f,
  @location(1) worldNormal: vec3f,
  @location(2) color: vec4f,
  @location(3) uv: vec2f,
  @location(4) receivesShadow: f32,
  @location(5) currentClip: vec4f,
  @location(6) previousClip: vec4f,
};

// Debug views (`DebugView` in Rust). Zero is the lit scene.
const DEBUG_BASE_COLOR: u32 = 1u;
const DEBUG_DIFFUSE_ALBEDO: u32 = 2u;
const DEBUG_SPECULAR_COLOR: u32 = 3u;
const DEBUG_ROUGHNESS: u32 = 4u;
const DEBUG_METALLIC: u32 = 5u;
const DEBUG_NORMALS: u32 = 6u;
const DEBUG_EMISSIVE: u32 = 7u;
const DEBUG_OCCLUSION: u32 = 8u;
const DEBUG_LIGHTING: u32 = 9u;
const DEBUG_SHADING_MODEL: u32 = 10u;
const DEBUG_SHADOW_CASCADES: u32 = 11u;
const DEBUG_UV: u32 = 12u;
const DEBUG_LIGHT_COMPLEXITY: u32 = 13u;

/**
 * Transform an instanced mesh vertex and carry its attributes to the fragment stage. The cofactor normal transform
 * keeps lighting correct under non-uniform scale; zero (unauthored) normals stay zero.
 */
@vertex fn vertexMain(vertex: MeshVertex, instance: Instance) -> SurfaceVarying {
  let model = instanceModel(instance);
  let world = model*vec4f(vertex.position, 1.0);
  let clips = motionClips(world, instancePreviousModel(instance)*vec4f(vertex.position, 1.0));

  var output: SurfaceVarying;
  output.position = frame.viewProjection*world;
  output.worldPosition = world.xyz;
  output.worldNormal = vec3f(0.0);
  if(dot(vertex.normal, vertex.normal) > 0.000001) {
    output.worldNormal = worldNormal(model, vertex.normal);
  }
  output.color = vertex.color*instance.tint;
  output.uv = vertex.uv;
  output.receivesShadow = instance.flags.x;
  output.currentClip = clips[0];
  output.previousClip = clips[1];
  return output;
}

/**
 * Lit color only: opaque and masked surfaces.
 */
@fragment fn fragmentMain(interpolated: SurfaceVarying, @builtin(front_facing) frontFacing: bool) -> @location(0) vec4f {
  let input = surfaceInput(interpolated, frontFacing);
  let surface = materialSurface(input);
  alphaTest(materialAlpha(surface));
  return vec4f(surfaceColor(surface, input, interpolated.receivesShadow), 1.0);
}

/**
 * Lit color with straight alpha, for blended surfaces and transparent instances. Masked materials still alpha-test.
 */
@fragment fn fragmentBlend(interpolated: SurfaceVarying, @builtin(front_facing) frontFacing: bool) -> @location(0) vec4f {
  let input = surfaceInput(interpolated, frontFacing);
  let surface = materialSurface(input);
  let alpha = materialAlpha(surface);
  alphaTest(alpha);
  return applyFog(vec4f(surfaceColor(surface, input, interpolated.receivesShadow), alpha), interpolated.worldPosition);
}

/**
 * Lit color plus the scene-effect motion attachment.
 */
@fragment fn fragmentTargets(interpolated: SurfaceVarying, @builtin(front_facing) frontFacing: bool) -> SceneTargets {
  let input = surfaceInput(interpolated, frontFacing);
  let surface = materialSurface(input);
  alphaTest(materialAlpha(surface));
  let color = vec4f(surfaceColor(surface, input, interpolated.receivesShadow), 1.0);
  return sceneTargets(color, interpolated.worldPosition, mat2x4f(interpolated.currentClip, interpolated.previousClip));
}

/**
 * The model's input for one fragment. Derivatives are taken here, which every entry point calls first, in uniform
 * control flow.
 */
fn surfaceInput(interpolated: SurfaceVarying, frontFacing: bool) -> SurfaceInput {
  var input: SurfaceInput;
  input.worldPosition = interpolated.worldPosition;
  input.positionDx = dpdx(interpolated.worldPosition);
  input.positionDy = dpdy(interpolated.worldPosition);
  input.uv = interpolated.uv;
  input.uvDx = dpdx(interpolated.uv);
  input.uvDy = dpdy(interpolated.uv);
  let face = cross(input.positionDx, input.positionDy);
  input.faceNormal = faceEye(face/max(length(face), 0.000001), interpolated.worldPosition);

  // Double-sided: back faces flip the authored normal; zero (unauthored) normals stay zero.
  input.vertexNormal = interpolated.worldNormal*select(-1.0, 1.0, frontFacing);
  input.viewDirection = normalize(frame.eye.xyz - interpolated.worldPosition);
  input.color = interpolated.color;
  input.frontFacing = frontFacing;
  return input;
}

/**
 * Discard a fragment that fails a masked material's alpha test.
 */
fn alphaTest(alpha: f32) {
  if((material.coverage.y == ALPHA_MODE_MASK) && (alpha < material.coverage.x)) {
    discard;
  }
}

/**
 * The surface's lit color, or the active debug view.
 */
fn surfaceColor(surface: MaterialSurface, input: SurfaceInput, receivesShadow: f32) -> vec3f {
  let view = u32(frame.debug.x);
  if(view == 0u) {
    return shadeSurface(surface, input.worldPosition, receivesShadow);
  }
  return debugColor(view, surface, input, receivesShadow);
}

/**
 * Light arriving from every direction: the frame's uniform ambient, the skylight, and image-based light.
 */
fn ambientLight() -> AmbientLight {
  var ambient: AmbientLight;
  ambient.uniformIrradiance = vec3f(frame.lighting.y);
  ambient.sky = frame.skylight.rgb*frame.skylight.a;
  ambient.ground = frame.groundSkylight.rgb*frame.skylight.a;
  ambient.environment = imageLighting.parameters.xyz;
  return ambient;
}

/**
 * The model's full response: emission, then (unless the model ignores light) direct light from the engine's light loop
 * and ambient and image-based light.
 */
fn shadeSurface(surface: MaterialSurface, worldPosition: vec3f, receivesShadow: f32) -> vec3f {
  var radiance = materialEmission(surface);
  if(!MATERIAL_RECEIVES_LIGHT) {
    return radiance;
  }
  let normal = materialAttributes(surface).normal;
  radiance+= directLighting(surface, worldPosition, normal, receivesShadow > 0.5);
  return radiance + materialIndirect(surface, ambientLight());
}

/**
 * One debug view of the surface. Colors show as stored (linear); scalar attributes show as gray.
 */
fn debugColor(view: u32, surface: MaterialSurface, input: SurfaceInput, receivesShadow: f32) -> vec3f {
  let attributes = materialAttributes(surface);
  switch view {
    case DEBUG_BASE_COLOR: {
      return mix(attributes.diffuseAlbedo, attributes.specularColor, attributes.metallic);
    }
    case DEBUG_DIFFUSE_ALBEDO: {
      return attributes.diffuseAlbedo;
    }
    case DEBUG_SPECULAR_COLOR: {
      return attributes.specularColor;
    }
    case DEBUG_ROUGHNESS: {
      return vec3f(attributes.roughness);
    }
    case DEBUG_METALLIC: {
      return vec3f(attributes.metallic);
    }
    case DEBUG_NORMALS: {
      return (attributes.normal*0.5) + 0.5;
    }
    case DEBUG_EMISSIVE: {
      return attributes.emissive;
    }
    case DEBUG_OCCLUSION: {
      return vec3f(attributes.occlusion);
    }
    case DEBUG_LIGHTING: {
      return lightingOnly(attributes.normal, input.worldPosition, receivesShadow);
    }
    case DEBUG_SHADING_MODEL: {
      return shadingModelColor(material.coverage.z);
    }
    case DEBUG_SHADOW_CASCADES: {
      let viewDepth = -(frame.view*vec4f(input.worldPosition, 1.0)).z;
      let lighting = lightingOnly(attributes.normal, input.worldPosition, receivesShadow);
      return cascadeColor(viewDepth)*(0.25 + (0.75*min(lighting, vec3f(1.0))));
    }
    case DEBUG_UV: {
      let checker = (u32(floor(input.uv.x*8.0)) + u32(floor(input.uv.y*8.0)))%2u;
      return vec3f(fract(input.uv), 0.0)*(0.6 + (0.4*f32(checker)));
    }
    case DEBUG_LIGHT_COMPLEXITY: {
      return lightComplexityColor(lightCluster(input.worldPosition).y);
    }
    default: {
      return shadeSurface(surface, input.worldPosition, receivesShadow);
    }
  }
}

/**
 * Irradiance a white, diffuse, double-sided surface would receive: the light loop without any model.
 */
fn lightingOnly(normal: vec3f, worldPosition: vec3f, receivesShadow: f32) -> vec3f {
  var surfaceNormal = normal;
  if(dot(surfaceNormal, surfaceNormal) < 0.000001) {
    surfaceNormal = vec3f(0.0, 1.0, 0.0);
  }
  return standardLighting(worldPosition, vec3f(1.0), surfaceNormal, receivesShadow);
}

/**
 * A distinct color for each shading model index.
 */
fn shadingModelColor(index: f32) -> vec3f {
  let palette = array<vec3f, 6>(
    vec3f(0.95, 0.75, 0.2),
    vec3f(0.25, 0.6, 0.95),
    vec3f(0.85, 0.85, 0.85),
    vec3f(0.9, 0.3, 0.6),
    vec3f(0.35, 0.85, 0.45),
    vec3f(0.6, 0.4, 0.95)
  );
  return palette[u32(index)%6u];
}

/**
 * A distinct color for the directional shadow cascade covering a view depth, white beyond the shadow distance.
 */
fn cascadeColor(viewDepth: f32) -> vec3f {
  let count = u32(shadows.counts.x);
  if((count == 0u) || (viewDepth >= shadows.directional.x)) {
    return vec3f(1.0);
  }
  let colors = array<vec3f, 4>(vec3f(1.0, 0.3, 0.3), vec3f(0.3, 1.0, 0.3), vec3f(0.35, 0.5, 1.0), vec3f(1.0, 0.9, 0.3));
  return colors[cascadeAt(viewDepth, count)];
}

/**
 * A heat color for how many point lights a cluster lists: black for none, then blue, green, yellow, and red at
 * LIGHT_COMPLEXITY_RED lights or more.
 */
fn lightComplexityColor(count: u32) -> vec3f {
  if(count == 0u) {
    return vec3f(0.0);
  }
  let heat = clamp(f32(count)/LIGHT_COMPLEXITY_RED, 0.0, 1.0);
  let ramp = array<vec3f, 4>(vec3f(0.1, 0.2, 1.0), vec3f(0.1, 0.9, 0.3), vec3f(1.0, 0.9, 0.1), vec3f(1.0, 0.1, 0.05));
  let position = heat*3.0;
  let index = min(u32(position), 2u);
  return mix(ramp[index], ramp[index + 1u], position - f32(index));
}

// Point lights in one cluster at which the light complexity view turns fully red.
const LIGHT_COMPLEXITY_RED: f32 = 32.0;
