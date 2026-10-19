// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Shared scene contract for every surface material: frame uniforms, mesh and instance inputs, shadow sampling, the
// engine's two lighting models, and the scene-effect attachments opaque materials write beside their color. Each
// material shader is this file followed by its own entry points.

const PI: f32 = 3.14159265359;
const MAX_POINT_LIGHTS: u32 = 32u;

struct PointLight {
  // World position and the distance at which the light reaches zero.
  positionRange: vec4f,
  // Linear RGB irradiance; A is the light's point-shadow slot plus one, or zero without a shadow.
  color: vec4f,
};

struct Frame {
  viewProjection: mat4x4f,
  view: mat4x4f,
  // XYZ eye position in world units; W is presentation time in seconds.
  eye: vec4f,
  // X point light count, Y ambient irradiance, ZW unused.
  lighting: vec4f,
  // XYZ direction toward the environment's primary light; W intensity.
  primaryDirection: vec4f,
  // RGB primary light color.
  primaryColor: vec4f,
  // RGB sky color overhead; A skylight intensity.
  skylight: vec4f,
  // RGB light the ground reflects up into the skylight's lower hemisphere; A unused.
  groundSkylight: vec4f,
  // Unjittered view-projection, and the previous frame's, for motion vectors.
  stableViewProjection: mat4x4f,
  previousViewProjection: mat4x4f,
  // XY change in temporal-antialiasing jitter since the previous frame, in UV units; ZW unused.
  motion: vec4f,
  lights: array<PointLight, 32>,
};

struct PointShadow {
  // XYZ light position; W range, the far plane of its face projections.
  positionRange: vec4f,
  // X near plane, Y source radius, Z receiver normal bias, W strength.
  parameters: vec4f,
};

struct Shadows {
  // X directional shadows (0 or 1), Y cascade count, Z blocker samples, W filter samples.
  counts: vec4f,
  // Far camera view depth of each cascade.
  cascadeSplits: vec4f,
  // Receiver normal offset per cascade: the configured bias, or 1.5 cascade texels when larger.
  cascadeBias: vec4f,
  cascades: array<mat4x4f, 4>,
  // X configured receiver normal bias, Y light angular radius, Z cascade blend fraction, W far-fade fraction.
  directional: vec4f,
  // X shadow distance, Y directional strength, Z shadowed point lights, W unused.
  extents: vec4f,
  points: array<PointShadow, 32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(0) var<uniform> shadows: Shadows;
@group(1) @binding(1) var directionalShadowMaps: texture_depth_2d_array;
@group(1) @binding(2) var pointShadowMaps: texture_depth_2d_array;
@group(1) @binding(3) var shadowDepthSampler: sampler;
@group(1) @binding(4) var shadowComparisonSampler: sampler_comparison;
// The same arrays again as plain floats for blocker-search depth reads; the GL backend allows one sampler per texture
// binding and cannot read depth textures without comparison.
@group(1) @binding(5) var directionalShadowDepths: texture_2d_array<f32>;
@group(1) @binding(6) var pointShadowDepths: texture_2d_array<f32>;

struct MeshVertex {
  @location(0) position: vec3f,
  @location(1) normal: vec3f,
  @location(2) color: vec4f,
  // X metallic, Y roughness, Z alpha cutoff, W alpha mode (0 opaque, 1 mask, 2 blend).
  @location(3) surface: vec4f,
  // RGB emission.
  @location(4) emissive: vec4f,
};

struct Instance {
  @location(5) model0: vec4f,
  @location(6) model1: vec4f,
  @location(7) model2: vec4f,
  @location(8) model3: vec4f,
  // RGB tint and opacity.
  @location(9) tint: vec4f,
  // X receives shadows (0 or 1), Y scene roughness for scene effects; remaining components are reserved.
  @location(10) flags: vec4f,
  // The previous frame's model matrix columns, for motion vectors (equal to the current matrix without history).
  @location(11) previous0: vec4f,
  @location(12) previous1: vec4f,
  @location(13) previous2: vec4f,
  @location(14) previous3: vec4f,
};

/**
 * Rebuild the instance's column-major model matrix from its four vertex-attribute columns.
 */
fn instanceModel(instance: Instance) -> mat4x4f {
  return mat4x4f(instance.model0, instance.model1, instance.model2, instance.model3);
}

/**
 * The instance's model matrix in the previous frame.
 */
fn instancePreviousModel(instance: Instance) -> mat4x4f {
  return mat4x4f(instance.previous0, instance.previous1, instance.previous2, instance.previous3);
}

/**
 * Transform a model-space normal into world space with the model's cofactor matrix.
 * The cofactor is the inverse transpose scaled by the determinant, so normalizing it handles non-uniform scale and
 * negative scale without a matrix inverse.
 */
fn worldNormal(model: mat4x4f, normal: vec3f) -> vec3f {
  let column0 = model[0].xyz;
  let column1 = model[1].xyz;
  let column2 = model[2].xyz;
  let cofactor = mat3x3f(cross(column1, column2), cross(column2, column0), cross(column0, column1));
  let transformed = cofactor*normal;
  return transformed/max(length(transformed), 0.000001);
}

/**
 * Orient a world-space normal toward the eye, giving double-sided surfaces a consistent front for lighting and bias.
 */
fn faceEye(normal: vec3f, worldPosition: vec3f) -> vec3f {
  if(dot(normal, frame.eye.xyz - worldPosition) < 0.0) {
    return -normal;
  }
  return normal;
}

// Shadow sampling, ported from the luma.gl 9.4.2 shadow module with the TypeScript engine's WebGPU-depth point-light
// adapter. Point lights keep their six faces in a 2D array, so the receiver selects and projects faces itself.

// SHADOW_COMPARE_BEGIN: the GL backend replaces this block with shadow-compare-gl.wgsl.
fn compareDirectionalShadow(coordinate: vec2f, layer: i32, reference: f32) -> f32 {
  return textureSampleCompareLevel(directionalShadowMaps, shadowComparisonSampler, coordinate, layer, reference);
}

fn comparePointShadow(coordinate: vec2f, layer: i32, reference: f32) -> f32 {
  return textureSampleCompareLevel(pointShadowMaps, shadowComparisonSampler, coordinate, layer, reference);
}
// SHADOW_COMPARE_END

fn shadowHash(position: vec3f) -> f32 {
  return fract(sin(dot(position, vec3f(12.9898, 78.233, 37.719)))*43758.5453);
}

/**
 * Golden-angle disk sample; `rotation` decorrelates neighboring pixels.
 */
fn shadowDiskSample(index: u32, count: u32, rotation: f32) -> vec2f {
  let fraction = (f32(index) + 0.5)/max(f32(count), 1.0);
  let angle = (f32(index)*2.39996323) + rotation;
  return sqrt(fraction)*vec2f(cos(angle), sin(angle));
}

/**
 * Project a world position to map coordinates (top-left origin) and zero-to-one depth.
 */
fn shadowProject(matrix: mat4x4f, worldPosition: vec3f) -> vec3f {
  let clip = matrix*vec4f(worldPosition, 1.0);
  let ndc = clip.xyz/max(abs(clip.w), 0.00001);
  return vec3f((ndc.x*0.5) + 0.5, 0.5 - (ndc.y*0.5), ndc.z);
}

fn shadowValidProjection(projected: vec3f) -> bool {
  return all(projected.xy >= vec2f(0.0)) && all(projected.xy <= vec2f(1.0)) && (projected.z >= 0.0) &&
    (projected.z <= 1.0);
}

fn cascadeIndex(viewDepth: f32) -> u32 {
  var index = 0u;
  let count = u32(shadows.counts.y);
  for(var candidate = 0u; candidate < 4u; candidate++) {
    if(candidate >= count) {
      break;
    }
    index = candidate;
    if(viewDepth <= shadows.cascadeSplits[candidate]) {
      break;
    }
  }
  return index;
}

/**
 * Percentage-closer soft shadow from one cascade: the blocker search sizes the penumbra from the light's angular
 * radius, then a rotated disk of comparisons filters it.
 */
fn cascadeVisibility(cascade: u32, receiver: vec3f, normal: vec3f, viewDepth: f32) -> f32 {
  let worldPosition = receiver + (normal*shadows.cascadeBias[cascade]);
  let projected = shadowProject(shadows.cascades[cascade], worldPosition);
  if(!shadowValidProjection(projected)) {
    return 1.0;
  }
  let texel = 1.0/f32(textureDimensions(directionalShadowDepths).x);
  let rotation = shadowHash(worldPosition)*2.0*PI;
  let searchRadius = max(texel, shadows.directional.y*viewDepth/max(shadows.extents.x, 0.001));
  let blockerSamples = u32(shadows.counts.z);
  let filterSamples = u32(shadows.counts.w);
  var blockerDepth = 0.0;
  var blockerCount = 0.0;
  for(var index = 0u; index < 24u; index++) {
    if(index >= blockerSamples) {
      break;
    }
    let offset = shadowDiskSample(index, blockerSamples, rotation)*searchRadius;
    let coordinate = clamp(projected.xy + offset, vec2f(0.0), vec2f(1.0));
    let sampleDepth = textureSampleLevel(directionalShadowDepths, shadowDepthSampler, coordinate, i32(cascade), 0.0).x;
    if(sampleDepth < projected.z) {
      blockerDepth+= sampleDepth;
      blockerCount+= 1.0;
    }
  }
  if(blockerCount == 0.0) {
    return 1.0;
  }
  let averageBlocker = blockerDepth/blockerCount;
  let penumbra = clamp((projected.z - averageBlocker)/max(abs(averageBlocker), 0.001), 0.0, 1.0);
  let filterRadius = max(texel, searchRadius*(1.0 + (penumbra*18.0)));
  var visibility = 0.0;
  for(var index = 0u; index < 48u; index++) {
    if(index >= filterSamples) {
      break;
    }
    let offset = shadowDiskSample(index, filterSamples, rotation)*filterRadius;
    let coordinate = clamp(projected.xy + offset, vec2f(0.0), vec2f(1.0));
    visibility+= compareDirectionalShadow(coordinate, i32(cascade), projected.z);
  }
  return visibility/max(f32(filterSamples), 1.0);
}

/**
 * Visibility of the environment's primary light. The receiver is displaced along its normal, cascades blend near
 * their far edges, and shadows fade out toward the shadow distance.
 */
fn directionalShadow(worldPosition: vec3f, normal: vec3f) -> f32 {
  let viewDepth = -(frame.view*vec4f(worldPosition, 1.0)).z;
  if((shadows.counts.x < 0.5) || (viewDepth <= 0.0) || (viewDepth >= shadows.extents.x)) {
    return 1.0;
  }
  let unit = normalize(normal);
  let cascade = cascadeIndex(viewDepth);
  var visibility = cascadeVisibility(cascade, worldPosition, unit, viewDepth);
  let count = u32(shadows.counts.y);
  if(cascade + 1u < count) {
    var cascadeStart = 0.0;
    if(cascade > 0u) {
      cascadeStart = shadows.cascadeSplits[cascade - 1u];
    }
    let cascadeEnd = shadows.cascadeSplits[cascade];
    let blendWidth = max((cascadeEnd - cascadeStart)*shadows.directional.z, 0.0001);
    let blend = smoothstep(cascadeEnd - blendWidth, cascadeEnd, viewDepth);
    if(blend > 0.0) {
      visibility = mix(visibility, cascadeVisibility(cascade + 1u, worldPosition, unit, viewDepth), blend);
    }
  }
  let fadeWidth = max(shadows.extents.x*shadows.directional.w, 0.0001);
  let farFade = smoothstep(shadows.extents.x - fadeWidth, shadows.extents.x, viewDepth);
  visibility = mix(visibility, 1.0, farFade);
  return mix(1.0, visibility, shadows.extents.y);
}

/**
 * The face whose 90-degree frustum contains a light-relative direction: +X, -X, +Y, -Y, +Z, -Z.
 */
fn pointFace(direction: vec3f) -> u32 {
  let magnitude = abs(direction);
  if((magnitude.x >= magnitude.y) && (magnitude.x >= magnitude.z)) {
    return select(1u, 0u, direction.x > 0.0);
  }
  if(magnitude.y >= magnitude.z) {
    return select(3u, 2u, direction.y > 0.0);
  }
  return select(5u, 4u, direction.z > 0.0);
}

/**
 * Face forward (column 0) and up (column 1) directions; must match `POINT_FACE_ORIENTATIONS` in shadows/mod.rs.
 */
fn pointFaceBasis(face: u32) -> mat2x3f {
  switch face {
    case 0u: { return mat2x3f(vec3f(1.0, 0.0, 0.0), vec3f(0.0, -1.0, 0.0)); }
    case 1u: { return mat2x3f(vec3f(-1.0, 0.0, 0.0), vec3f(0.0, -1.0, 0.0)); }
    case 2u: { return mat2x3f(vec3f(0.0, 1.0, 0.0), vec3f(0.0, 0.0, 1.0)); }
    case 3u: { return mat2x3f(vec3f(0.0, -1.0, 0.0), vec3f(0.0, 0.0, -1.0)); }
    case 4u: { return mat2x3f(vec3f(0.0, 0.0, 1.0), vec3f(0.0, -1.0, 0.0)); }
    default: { return mat2x3f(vec3f(0.0, 0.0, -1.0), vec3f(0.0, -1.0, 0.0)); }
  }
}

/**
 * Map coordinates of a direction within its face, matching the face's 90-degree perspective raster.
 */
fn pointFaceCoordinate(direction: vec3f, face: u32) -> vec2f {
  let basis = pointFaceBasis(face);
  let right = cross(basis[0], basis[1]);
  let major = max(dot(basis[0], direction), 0.000001);
  return vec2f((dot(right, direction)/major*0.5) + 0.5, 0.5 - (dot(basis[1], direction)/major*0.5));
}

/**
 * Zero-to-one perspective depth of a point at `majorDistance` along a face's forward axis.
 */
fn pointReferenceDepth(majorDistance: f32, nearPlane: f32, farPlane: f32) -> f32 {
  return (farPlane/(farPlane - nearPlane)) - ((farPlane*nearPlane)/((farPlane - nearPlane)*majorDistance));
}

/**
 * Receiver depth along one filter ray, intersected with the biased receiver plane. A center-only reference falsely
 * treats sloped receivers as blockers across a wide kernel. Rays parallel to or outside the receiver's light frustum
 * return zero, which compares as unoccluded.
 */
fn pointPlaneDepth(direction: vec3f, fromLight: vec3f, normal: vec3f, nearPlane: f32, farPlane: f32) -> f32 {
  let denominator = dot(normal, direction);
  if(abs(denominator) < 0.00001) {
    return 0.0;
  }
  let distance = dot(normal, fromLight)/denominator;
  let majorDistance = distance*max(max(abs(direction.x), abs(direction.y)), abs(direction.z));
  if((majorDistance <= nearPlane) || (majorDistance >= farPlane)) {
    return 0.0;
  }
  return pointReferenceDepth(majorDistance, nearPlane, farPlane);
}

/**
 * Visibility of one point light from its shadow slot (negative for none), with soft shadows sized by the light's
 * source radius.
 */
fn pointShadow(slot: i32, worldPosition: vec3f, normal: vec3f) -> f32 {
  if((slot < 0) || (slot >= i32(shadows.extents.z))) {
    return 1.0;
  }
  let light = shadows.points[slot];
  let nearPlane = light.parameters.x;
  let farPlane = light.positionRange.w;
  let unit = normalize(normal);
  let fromLight = worldPosition + (unit*light.parameters.z) - light.positionRange.xyz;
  let distanceToLight = length(fromLight);
  if((distanceToLight >= farPlane) || (distanceToLight <= nearPlane)) {
    return 1.0;
  }
  let direction = fromLight/distanceToLight;
  let majorDistance = max(max(abs(fromLight.x), abs(fromLight.y)), abs(fromLight.z));
  let referenceDepth = pointReferenceDepth(majorDistance, nearPlane, farPlane);
  let searchRadius = light.parameters.y/max(distanceToLight, 0.001);
  let rotation = shadowHash(worldPosition)*2.0*PI;
  let reference = select(vec3f(0.0, 1.0, 0.0), vec3f(0.0, 0.0, 1.0), abs(direction.y) > 0.99);
  let tangent = normalize(cross(direction, reference));
  let bitangent = normalize(cross(direction, tangent));
  let firstLayer = slot*6;
  let blockerSamples = u32(shadows.counts.z);
  let filterSamples = u32(shadows.counts.w);
  var blockerDepth = 0.0;
  var blockerCount = 0.0;
  for(var index = 0u; index < 24u; index++) {
    if(index >= blockerSamples) {
      break;
    }
    let disk = shadowDiskSample(index, blockerSamples, rotation)*searchRadius;
    let sampleDirection = normalize(direction + (tangent*disk.x) + (bitangent*disk.y));
    let receiverDepth = pointPlaneDepth(sampleDirection, fromLight, unit, nearPlane, farPlane);
    let face = pointFace(sampleDirection);
    let coordinate = pointFaceCoordinate(sampleDirection, face);
    let sampleDepth =
      textureSampleLevel(pointShadowDepths, shadowDepthSampler, coordinate, firstLayer + i32(face), 0.0).x;
    if(sampleDepth < receiverDepth) {
      blockerDepth+= sampleDepth;
      blockerCount+= 1.0;
    }
  }
  if(blockerCount == 0.0) {
    return 1.0;
  }
  let averageBlocker = blockerDepth/blockerCount;
  let penumbra = clamp((referenceDepth - averageBlocker)/max(abs(averageBlocker), 0.001), 0.0, 1.0);
  let filterRadius = searchRadius*(1.0 + (penumbra*12.0));
  var visibility = 0.0;
  for(var index = 0u; index < 48u; index++) {
    if(index >= filterSamples) {
      break;
    }
    let disk = shadowDiskSample(index, filterSamples, rotation)*filterRadius;
    let sampleDirection = normalize(direction + (tangent*disk.x) + (bitangent*disk.y));
    let receiverDepth = pointPlaneDepth(sampleDirection, fromLight, unit, nearPlane, farPlane);
    let face = pointFace(sampleDirection);
    let coordinate = pointFaceCoordinate(sampleDirection, face);
    visibility+= comparePointShadow(coordinate, firstLayer + i32(face), receiverDepth);
  }
  return mix(1.0, visibility/max(f32(filterSamples), 1.0), light.parameters.w);
}

/**
 * Skylight irradiance on a surface: the sky overhead blended toward the ground's reflected light as the normal tilts
 * down, the exact irradiance of two uniform hemispheres (`Skylight::irradiance` in joy-core).
 */
fn skylightIrradiance(normal: vec3f) -> vec3f {
  let skyWeight = clamp(0.5 + (0.5*normal.y), 0.0, 1.0);
  return mix(frame.groundSkylight.rgb, frame.skylight.rgb, skyWeight)*frame.skylight.a;
}

/**
 * The engine's standard flat lighting: ambient plus the sky-and-ground skylight, a double-sided primary light, and
 * point lights. Matches the TypeScript scene path; diffuse terms use |n.l| without a 1/pi factor so existing scenes
 * keep their authored brightness.
 */
fn standardLighting(worldPosition: vec3f, color: vec3f, normal: vec3f, receivesShadow: f32) -> vec3f {
  var irradiance = vec3f(frame.lighting.y) + skylightIrradiance(normal);

  var sunVisibility = 1.0;
  if(receivesShadow > 0.5) {
    sunVisibility = directionalShadow(worldPosition, normal);
  }
  let primaryDiffuse = abs(dot(normal, frame.primaryDirection.xyz));
  irradiance+= frame.primaryColor.rgb*frame.primaryDirection.w*primaryDiffuse*sunVisibility;

  let lightCount = min(u32(frame.lighting.x), MAX_POINT_LIGHTS);
  for(var index = 0u; index < lightCount; index++) {
    let light = frame.lights[index];
    let offset = light.positionRange.xyz - worldPosition;
    let distance = length(offset);
    let direction = offset/max(distance, 0.000001);
    let reach = max(0.0, 1.0 - (distance/light.positionRange.w));
    var visibility = 1.0;
    if(receivesShadow > 0.5) {
      visibility = pointShadow(i32(light.color.a) - 1, worldPosition, normal);
    }
    irradiance+= light.color.rgb*(reach*reach)*abs(dot(normal, direction))*visibility;
  }

  return color*irradiance;
}

/**
 * Metallic-roughness response to one light: GGX specular with energy-conserving Lambertian diffuse.
 * Matches the TypeScript PBR material pass; the caller applies light color, attenuation, and visibility.
 */
fn pbrDirect(base: vec3f, metallic: f32, roughness: f32, n: vec3f, v: vec3f, l: vec3f) -> vec3f {
  let halfDirection = v + l;
  let h = halfDirection/max(length(halfDirection), 0.000001);
  let nv = max(dot(n, v), 0.0001);
  let nl = max(dot(n, l), 0.0);
  let nh = max(dot(n, h), 0.0);
  let vh = max(dot(v, h), 0.0);
  let a2 = pow(roughness, 4.0);
  let denominator = (nh*nh*(a2 - 1.0)) + 1.0;
  let distribution = a2/max(PI*denominator*denominator, 0.000000000001);
  let visibility = 0.5/max((nl*sqrt((nv*nv*(1.0 - a2)) + a2)) + (nv*sqrt((nl*nl*(1.0 - a2)) + a2)), 0.0001);
  let f0 = mix(vec3f(0.04), base, metallic);
  let fresnel = f0 + ((vec3f(1.0) - f0)*pow(1.0 - vh, 5.0));
  let diffuse = (vec3f(1.0) - fresnel)*(1.0 - metallic)*base/PI;

  return (diffuse + (distribution*visibility*fresnel))*nl;
}

// Scene-effect attachments. With effects that need them, opaque materials write view-space normals with roughness and
// both motion vectors beside their color in the one scene pass, instead of rasterizing opaque geometry again.

struct SceneTargets {
  @location(0) color: vec4f,
  // View-space normal encoded as `n*0.5 + 0.5`, oriented toward the eye; A scene roughness.
  @location(1) normalRoughness: vec4f,
  // Screen-space motion in UV units without jitter (antialiasing and motion blur) and with it (lighting effects).
  @location(2) motion: vec4f,
  @location(3) jitteredMotion: vec4f,
};

/**
 * Clip positions for motion vectors: this frame's unjittered camera and the previous frame's camera and transform.
 */
fn motionClips(world: vec4f, previousWorld: vec4f) -> mat2x4f {
  return mat2x4f(frame.stableViewProjection*world, frame.previousViewProjection*previousWorld);
}

/**
 * Every scene target for one opaque fragment. `surface` is `cross(dpdx(worldPosition), dpdy(worldPosition))`, taken by
 * the caller in uniform control flow; the normal is the geometric plane, so every material encodes the same surface.
 */
fn sceneTargets(color: vec4f, worldPosition: vec3f, surface: vec3f, roughness: f32, clips: mat2x4f) -> SceneTargets {
  let worldNormal = surface/max(length(surface), 0.000001);
  let transformed = (frame.view*vec4f(worldNormal, 0.0)).xyz;
  var viewNormal = transformed/max(length(transformed), 0.000001);
  let viewPosition = (frame.view*vec4f(worldPosition, 1.0)).xyz;

  // Lighting is double-sided. Orient the visible side toward the eye while keeping signed components: ambient
  // occlusion and reflection rays depend on the complete direction.
  if(dot(viewNormal, -viewPosition) < 0.0) {
    viewNormal = -viewNormal;
  }

  // History behind the eye is not a valid correspondence and must never produce NaNs. Framebuffer UVs run downward
  // while clip-space Y runs upward.
  let current = clips[0];
  let previous = clips[1];
  var velocity = vec2f(0.0);
  if((current.w > 0.000001) && (previous.w > 0.000001)) {
    velocity = ((current.xy/current.w) - (previous.xy/previous.w))*vec2f(0.5, -0.5);
  }

  var targets: SceneTargets;
  targets.color = color;
  targets.normalRoughness = vec4f((viewNormal*0.5) + 0.5, clamp(roughness, 0.0, 1.0));
  targets.motion = vec4f(velocity, 0.0, 1.0);
  targets.jitteredMotion = vec4f(velocity + frame.motion.xy, 0.0, 1.0);
  return targets;
}
