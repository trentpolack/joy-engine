// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Shadow receivers for every lit pass: the primary light's cascades and the point lights' atlas tiles (see
// `shadows/mod.rs` for how both are rendered). Appended to scene-common.wgsl, whose frame uniforms it reads.
//
// Both kinds filter the same way. A blocker search over the region the light's size could shadow finds the average
// occluder depth, which sets the penumbra width (percentage-closer soft shadows); a rotated Vogel disk of hardware
// comparisons then averages visibility over that width. Receivers move off their surface by a few texels along the
// normal, and every tap compares against the receiver's own plane rather than the center depth, so wide kernels do not
// shadow sloped surfaces with themselves. Filter widths are measured in texels of the map that is read, so one setting
// behaves the same in a near cascade, a far cascade, and a small atlas tile.

// Most blocker-search and filter taps any quality preset takes.
const SHADOW_MAX_BLOCKER_SAMPLES: u32 = 32u;
const SHADOW_MAX_FILTER_SAMPLES: u32 = 64u;

// Penumbra limits in texels of the map being read: the narrowest kernel that still hides texel edges and the widest
// worth its taps.
const SHADOW_MIN_FILTER_TEXELS: f32 = 1.5;
const SHADOW_MAX_FILTER_TEXELS: f32 = 24.0;

// Steepest receiver slope the plane bias follows (about 80 degrees from facing the light); steeper planes would push
// the reference behind real occluders.
const SHADOW_MAX_RECEIVER_SLOPE: f32 = 6.0;

// The angle between successive Vogel disk taps: 2 pi (2 - phi).
const GOLDEN_ANGLE: f32 = 2.39996323;

struct Cascade {
  // World to cascade clip space; the projection is orthographic.
  matrix: mat4x4f,
  // X far view depth the cascade covers, Y world size of one texel, Z world depth its depth range spans, W receiver
  // offset along the normal.
  extent: vec4f,
};

struct PointShadow {
  // XYZ light position; W range, the far plane of its face projections.
  positionRange: vec4f,
  // X face near plane, Y light source radius, Z minimum receiver offset along the normal, W strength.
  parameters: vec4f,
  // Atlas UV origin of each face's tile, two faces per entry: (+X, -X), (+Y, -Y), (+Z, -Z).
  tiles: array<vec4f, 3>,
  // X tile size in atlas UV, Y tile size in texels, ZW unused.
  tile: vec4f,
};

struct Shadows {
  // X cascades (0 without directional shadows), Y blocker-search taps, Z filter taps, W shadowed point lights.
  counts: vec4f,
  // X shadow distance, Y directional strength, Z tangent of the light's angular radius, W fraction of each cascade
  // blended into the next.
  directional: vec4f,
  // X fraction of the shadow distance over which shadows fade out, YZW unused.
  parameters: vec4f,
  cascades: array<Cascade, 4>,
  points: array<PointShadow, 32>,
};

@group(1) @binding(0) var<uniform> shadows: Shadows;
@group(1) @binding(1) var cascadeMaps: texture_depth_2d_array;
@group(1) @binding(2) var pointShadowAtlas: texture_depth_2d;
@group(1) @binding(3) var shadowComparisonSampler: sampler_comparison;
// The same maps again as plain floats for blocker-search reads (and WebGL 2's comparisons), always read through the
// nearest sampler below: the GL backend allows one sampler per texture binding, cannot load depth textures directly,
// and treats a depth texture read without a nearest sampler or comparison as incomplete (every read returns zero).
@group(1) @binding(4) var cascadeDepths: texture_2d_array<f32>;
@group(1) @binding(5) var pointShadowDepths: texture_2d<f32>;
@group(1) @binding(6) var shadowDepthSampler: sampler;

// SHADOW_COMPARE_BEGIN: the GL backend replaces this block with shadow-compare-gl.wgsl.
fn compareCascade(coordinate: vec2f, cascade: u32, reference: f32) -> f32 {
  return textureSampleCompareLevel(cascadeMaps, shadowComparisonSampler, coordinate, cascade, reference);
}

fn compareAtlas(coordinate: vec2f, reference: f32) -> f32 {
  return textureSampleCompareLevel(pointShadowAtlas, shadowComparisonSampler, coordinate, reference);
}
// SHADOW_COMPARE_END

// ---------------------------------------------------------------------------------------------------------------------
// Directional cascades
// ---------------------------------------------------------------------------------------------------------------------

/**
 * Visibility of the primary light: the covering cascade, blended into the next near its far edge and faded out toward
 * the shadow distance.
 */
fn directionalShadow(worldPosition: vec3f, normal: vec3f) -> f32 {
  let count = u32(shadows.counts.x);
  let shadowDistance = shadows.directional.x;
  let viewDepth = -(frame.view*vec4f(worldPosition, 1.0)).z;
  if((count == 0u) || (viewDepth <= 0.0) || (viewDepth >= shadowDistance)) {
    return 1.0;
  }

  let unitNormal = safeShadowNormal(normal);
  let rotation = shadowNoise(worldPosition)*2.0*PI;
  let cascade = cascadeAt(viewDepth, count);
  var visibility = cascadeVisibility(cascade, worldPosition, unitNormal, rotation);

  // Blend toward the next cascade across the last part of this one, so the resolution change has no seam.
  if(cascade + 1u < count) {
    let cascadeEnd = shadows.cascades[cascade].extent.x;
    var cascadeStart = 0.0;
    if(cascade > 0u) {
      cascadeStart = shadows.cascades[cascade - 1u].extent.x;
    }
    let blendWidth = max((cascadeEnd - cascadeStart)*shadows.directional.w, 0.0001);
    let blend = smoothstep(cascadeEnd - blendWidth, cascadeEnd, viewDepth);
    if(blend > 0.0) {
      visibility = mix(visibility, cascadeVisibility(cascade + 1u, worldPosition, unitNormal, rotation), blend);
    }
  }

  let fadeStart = shadowDistance*(1.0 - shadows.parameters.x);
  visibility = mix(visibility, 1.0, smoothstep(fadeStart, shadowDistance, viewDepth));
  return mix(1.0, visibility, shadows.directional.y);
}

/**
 * The first cascade whose far depth reaches `viewDepth` (the last cascade beyond them all).
 */
fn cascadeAt(viewDepth: f32, count: u32) -> u32 {
  for(var cascade = 0u; cascade + 1u < count; cascade++) {
    if(viewDepth <= shadows.cascades[cascade].extent.x) {
      return cascade;
    }
  }
  return count - 1u;
}

/**
 * Soft visibility from one cascade. Penumbrae widen with the distance from blocker to receiver times the tangent of
 * the light's angular radius, as a disc light such as the sun casts them.
 */
fn cascadeVisibility(cascade: u32, worldPosition: vec3f, normal: vec3f, rotation: f32) -> f32 {
  let data = shadows.cascades[cascade];
  let texelWorld = data.extent.y;
  let depthWorld = data.extent.z;
  let receiver = projectToMap(data.matrix, worldPosition + (normal*data.extent.w));
  if(!insideMap(receiver)) {
    return 1.0;
  }
  let mapSize = vec2f(textureDimensions(cascadeDepths).xy);
  let texelUv = 1.0/mapSize.x;
  let slope = receiverPlaneSlope(data.matrix, normal, texelWorld/(texelUv*depthWorld));
  let tangentAngle = shadows.directional.z;

  // Blockers anywhere between the receiver and the light could shadow it; the search covers the widest penumbra they
  // could cast.
  let searchWorld = receiver.z*depthWorld*tangentAngle;
  let searchTexels = clamp(searchWorld/texelWorld, SHADOW_MIN_FILTER_TEXELS, SHADOW_MAX_FILTER_TEXELS);
  let blockerTaps = min(u32(shadows.counts.y), SHADOW_MAX_BLOCKER_SAMPLES);
  var blockerSum = 0.0;
  var blockerCount = 0.0;
  for(var tap = 0u; tap < blockerTaps; tap++) {
    let offset = vogelDisk(tap, blockerTaps, rotation)*searchTexels*texelUv;
    let reference = receiver.z + dot(slope, offset);
    let texel = clamp(vec2i((receiver.xy + offset)*mapSize), vec2i(0), vec2i(mapSize) - vec2i(1));
    let depth = cascadeDepth(texel, cascade);
    if(depth < reference) {
      blockerSum+= depth;
      blockerCount+= 1.0;
    }
  }
  if(blockerCount == 0.0) {
    return 1.0;
  }

  let blockerDepth = blockerSum/blockerCount;
  let penumbraWorld = max(receiver.z - blockerDepth, 0.0)*depthWorld*tangentAngle;
  let filterTexels = clamp(penumbraWorld/texelWorld, SHADOW_MIN_FILTER_TEXELS, SHADOW_MAX_FILTER_TEXELS);
  let filterTaps = max(min(u32(shadows.counts.z), SHADOW_MAX_FILTER_SAMPLES), 1u);
  var visibility = 0.0;
  for(var tap = 0u; tap < filterTaps; tap++) {
    let offset = vogelDisk(tap, filterTaps, rotation)*filterTexels*texelUv;
    let coordinate = clamp(receiver.xy + offset, vec2f(0.0), vec2f(1.0));
    visibility+= compareCascade(coordinate, cascade, receiver.z + dot(slope, offset));
  }
  return visibility/f32(filterTaps);
}

/**
 * One filtered comparison in the covering cascade, for participating media where soft filtering is unnecessary.
 */
fn directionalShadowSharp(worldPosition: vec3f) -> f32 {
  let count = u32(shadows.counts.x);
  let viewDepth = -(frame.view*vec4f(worldPosition, 1.0)).z;
  if((count == 0u) || (viewDepth <= 0.0) || (viewDepth >= shadows.directional.x)) {
    return 1.0;
  }
  let cascade = cascadeAt(viewDepth, count);
  let receiver = projectToMap(shadows.cascades[cascade].matrix, worldPosition);
  if(!insideMap(receiver)) {
    return 1.0;
  }
  return mix(1.0, compareCascade(receiver.xy, cascade, receiver.z), shadows.directional.y);
}

// ---------------------------------------------------------------------------------------------------------------------
// Point lights
// ---------------------------------------------------------------------------------------------------------------------

/**
 * Visibility of one point light from its shadow slot (negative for none). Each tap picks the cube face its direction
 * falls in, so filters cross face edges without seams.
 */
fn pointShadow(slot: i32, worldPosition: vec3f, normal: vec3f) -> f32 {
  if((slot < 0) || (slot >= i32(shadows.counts.w))) {
    return 1.0;
  }
  let light = shadows.points[slot];
  let position = light.positionRange.xyz;
  let farPlane = light.positionRange.w;
  let nearPlane = light.parameters.x;
  let tileTexels = light.tile.y;
  let unitNormal = safeShadowNormal(normal);

  // A face texel spans 2 / tileTexels radians near the face center; offset the receiver by a few of them at its
  // distance, or the configured minimum.
  let texelAngle = 2.0/tileTexels;
  let offset = max(light.parameters.z, POINT_TEXEL_OFFSET*texelAngle*distance(worldPosition, position));
  let fromLight = worldPosition + (unitNormal*offset) - position;
  let receiverDistance = length(fromLight);
  if((receiverDistance >= farPlane) || (receiverDistance <= nearPlane)) {
    return 1.0;
  }
  let direction = fromLight/receiverDistance;
  let reference = select(vec3f(0.0, 1.0, 0.0), vec3f(1.0, 0.0, 0.0), abs(direction.y) > 0.99);
  let tangent = normalize(cross(direction, reference));
  let bitangent = cross(direction, tangent);
  let rotation = shadowNoise(worldPosition)*2.0*PI;
  let receiverMajor = majorAxisDistance(fromLight);

  // Blocker search across the cone the light's disc subtends from the receiver.
  let lightRadius = light.parameters.y;
  let minimumAngle = SHADOW_MIN_FILTER_TEXELS*texelAngle;
  let maximumAngle = SHADOW_MAX_FILTER_TEXELS*texelAngle;
  let searchAngle = clamp(lightRadius/receiverDistance, minimumAngle, maximumAngle);
  let blockerTaps = min(u32(shadows.counts.y), SHADOW_MAX_BLOCKER_SAMPLES);
  var blockerSum = 0.0;
  var blockerCount = 0.0;
  for(var tap = 0u; tap < blockerTaps; tap++) {
    let disk = vogelDisk(tap, blockerTaps, rotation)*searchAngle;
    let sampleDirection = normalize(direction + (tangent*disk.x) + (bitangent*disk.y));
    let receiverDepth = pointPlaneDepth(sampleDirection, fromLight, unitNormal, nearPlane, farPlane);
    let atlasSize = vec2f(textureDimensions(pointShadowDepths));
    let depth = atlasDepth(vec2i(pointAtlasCoordinate(slot, sampleDirection)*atlasSize));
    if(depth < receiverDepth) {
      blockerSum+= pointLinearDistance(depth, nearPlane, farPlane);
      blockerCount+= 1.0;
    }
  }
  if(blockerCount == 0.0) {
    return 1.0;
  }

  // The penumbra a disc light casts past a blocker, seen from the light: radius * (receiver - blocker) / (blocker *
  // receiver).
  let blockerMajor = max(blockerSum/blockerCount, nearPlane);
  let penumbraAngle = lightRadius*max(receiverMajor - blockerMajor, 0.0)/(blockerMajor*receiverMajor);
  let filterAngle = clamp(penumbraAngle, minimumAngle, maximumAngle);
  let filterTaps = max(min(u32(shadows.counts.z), SHADOW_MAX_FILTER_SAMPLES), 1u);
  var visibility = 0.0;
  for(var tap = 0u; tap < filterTaps; tap++) {
    let disk = vogelDisk(tap, filterTaps, rotation)*filterAngle;
    let sampleDirection = normalize(direction + (tangent*disk.x) + (bitangent*disk.y));
    let receiverDepth = pointPlaneDepth(sampleDirection, fromLight, unitNormal, nearPlane, farPlane);
    visibility+= compareAtlas(pointAtlasCoordinate(slot, sampleDirection), receiverDepth);
  }
  return mix(1.0, visibility/f32(filterTaps), light.parameters.w);
}

/**
 * One filtered comparison in a point light's tile, for participating media.
 */
fn pointShadowSharp(slot: i32, worldPosition: vec3f) -> f32 {
  if((slot < 0) || (slot >= i32(shadows.counts.w))) {
    return 1.0;
  }
  let light = shadows.points[slot];
  let fromLight = worldPosition - light.positionRange.xyz;
  let receiverDistance = length(fromLight);
  if((receiverDistance >= light.positionRange.w) || (receiverDistance <= light.parameters.x)) {
    return 1.0;
  }
  let depth = pointPerspectiveDepth(majorAxisDistance(fromLight), light.parameters.x, light.positionRange.w);
  let visibility = compareAtlas(pointAtlasCoordinate(slot, fromLight/receiverDistance), depth);
  return mix(1.0, visibility, light.parameters.w);
}

// Receiver offset for point lights in face texels at the receiver's distance.
const POINT_TEXEL_OFFSET: f32 = 1.5;

/**
 * The cube face a light-relative direction falls in, in tile order: +X, -X, +Y, -Y, +Z, -Z.
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
 * Face-local map coordinates (top-left origin) of a direction inside `face`. The right, up, and forward axes must match
 * `POINT_FACE_ORIENTATIONS` in shadows/mod.rs, where forward and up are listed and right is forward x up.
 */
fn pointFaceCoordinate(direction: vec3f, face: u32) -> vec2f {
  var forward = vec3f(0.0);
  var up = vec3f(0.0, -1.0, 0.0);
  switch face {
    case 0u: {
      forward = vec3f(1.0, 0.0, 0.0);
    }
    case 1u: {
      forward = vec3f(-1.0, 0.0, 0.0);
    }
    case 2u: {
      forward = vec3f(0.0, 1.0, 0.0);
      up = vec3f(0.0, 0.0, 1.0);
    }
    case 3u: {
      forward = vec3f(0.0, -1.0, 0.0);
      up = vec3f(0.0, 0.0, -1.0);
    }
    case 4u: {
      forward = vec3f(0.0, 0.0, 1.0);
    }
    default: {
      forward = vec3f(0.0, 0.0, -1.0);
    }
  }
  let right = cross(forward, up);
  let along = max(dot(forward, direction), 0.000001);
  return vec2f((dot(right, direction)/along*0.5) + 0.5, 0.5 - (dot(up, direction)/along*0.5));
}

/**
 * Atlas coordinates of a light-relative direction in its shadow slot's tiles, kept half a texel inside the face's
 * tile so filtering never reads a neighbor.
 */
fn pointAtlasCoordinate(slot: i32, direction: vec3f) -> vec2f {
  let face = pointFace(direction);
  let tileTexels = shadows.points[slot].tile.y;
  let inset = 0.5/tileTexels;
  let local = clamp(pointFaceCoordinate(direction, face), vec2f(inset), vec2f(1.0 - inset));
  let pair = shadows.points[slot].tiles[face/2u];
  let origin = select(pair.xy, pair.zw, (face%2u) == 1u);
  return origin + (local*shadows.points[slot].tile.x);
}

/**
 * The largest component of a light-relative offset: the distance along the forward axis of the face it falls in.
 */
fn majorAxisDistance(offset: vec3f) -> f32 {
  let magnitude = abs(offset);
  return max(max(magnitude.x, magnitude.y), magnitude.z);
}

/**
 * Zero-to-one depth a face's perspective projection stores for a point `major` units along its forward axis.
 */
fn pointPerspectiveDepth(major: f32, nearPlane: f32, farPlane: f32) -> f32 {
  return (farPlane*(major - nearPlane))/(major*(farPlane - nearPlane));
}

/**
 * The forward-axis distance a stored face depth represents; inverts `pointPerspectiveDepth`.
 */
fn pointLinearDistance(depth: f32, nearPlane: f32, farPlane: f32) -> f32 {
  return (farPlane*nearPlane)/max(farPlane - (depth*(farPlane - nearPlane)), 0.000001);
}

/**
 * The stored-depth reference of the receiver's plane along one filter ray from the light. Rays parallel to the plane
 * or meeting it outside the light's reach compare as lit.
 */
fn pointPlaneDepth(direction: vec3f, fromLight: vec3f, normal: vec3f, nearPlane: f32, farPlane: f32) -> f32 {
  let alignment = dot(normal, direction);
  if(abs(alignment) < 0.00001) {
    return 0.0;
  }
  let along = dot(normal, fromLight)/alignment;
  let major = along*majorAxisDistance(direction);
  if((major <= nearPlane) || (major >= farPlane)) {
    return 0.0;
  }
  return pointPerspectiveDepth(major, nearPlane, farPlane);
}

// ---------------------------------------------------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------------------------------------------------

/**
 * The depth one cascade texel stores, read at the texel's center through the nearest sampler.
 */
fn cascadeDepth(texel: vec2i, cascade: u32) -> f32 {
  let coordinate = (vec2f(texel) + vec2f(0.5))/vec2f(textureDimensions(cascadeDepths).xy);
  return textureSampleLevel(cascadeDepths, shadowDepthSampler, coordinate, cascade, 0.0).x;
}

/**
 * The depth one atlas texel stores, read at the texel's center through the nearest sampler.
 */
fn atlasDepth(texel: vec2i) -> f32 {
  let coordinate = (vec2f(texel) + vec2f(0.5))/vec2f(textureDimensions(pointShadowDepths));
  return textureSampleLevel(pointShadowDepths, shadowDepthSampler, coordinate, 0.0).x;
}

/**
 * Map coordinates (top-left origin) and zero-to-one depth of a world position under an orthographic light matrix.
 */
fn projectToMap(matrix: mat4x4f, worldPosition: vec3f) -> vec3f {
  let clip = matrix*vec4f(worldPosition, 1.0);
  return vec3f((clip.x*0.5) + 0.5, 0.5 - (clip.y*0.5), clip.z);
}

fn insideMap(projected: vec3f) -> bool {
  return all(projected >= vec3f(0.0)) && all(projected <= vec3f(1.0));
}

/**
 * How map depth changes per unit of map coordinates across the receiver's plane, so each filter tap compares against
 * the plane under it. `unitSlope` is the depth change per map unit of a plane at 45 degrees to the light (world units
 * per map unit over the world depth the map spans); slopes are limited to SHADOW_MAX_RECEIVER_SLOPE times it.
 */
fn receiverPlaneSlope(matrix: mat4x4f, normal: vec3f, unitSlope: f32) -> vec2f {
  let helper = select(vec3f(0.0, 1.0, 0.0), vec3f(1.0, 0.0, 0.0), abs(normal.y) > 0.9);
  let tangent = normalize(cross(normal, helper));
  let bitangent = cross(normal, tangent);
  // Both plane directions in map units: U right, V down, depth forward.
  let alongTangent = (matrix*vec4f(tangent, 0.0)).xyz*vec3f(0.5, -0.5, 1.0);
  let alongBitangent = (matrix*vec4f(bitangent, 0.0)).xyz*vec3f(0.5, -0.5, 1.0);
  let determinant = (alongTangent.x*alongBitangent.y) - (alongTangent.y*alongBitangent.x);
  if(abs(determinant) < 0.0000000001) {
    return vec2f(0.0);
  }
  let slope = vec2f(
    ((alongTangent.z*alongBitangent.y) - (alongBitangent.z*alongTangent.y))/determinant,
    ((alongBitangent.z*alongTangent.x) - (alongTangent.z*alongBitangent.x))/determinant
  );
  let limit = SHADOW_MAX_RECEIVER_SLOPE*unitSlope;
  let magnitude = length(slope);
  if(magnitude > limit) {
    return slope*(limit/magnitude);
  }
  return slope;
}

/**
 * Vogel disk tap `index` of `count`: evenly spread over the unit disk, turned by `rotation`.
 */
fn vogelDisk(index: u32, count: u32, rotation: f32) -> vec2f {
  let radius = sqrt((f32(index) + 0.5)/f32(max(count, 1u)));
  let angle = (f32(index)*GOLDEN_ANGLE) + rotation;
  return radius*vec2f(cos(angle), sin(angle));
}

/**
 * Interleaved gradient noise (Jimenez 2014) at the pixel a position projects to, shifted every frame while temporal
 * antialiasing accumulates, so neighboring pixels and frames rotate their filter disks differently.
 */
fn shadowNoise(worldPosition: vec3f) -> f32 {
  let clip = frame.stableViewProjection*vec4f(worldPosition, 1.0);
  let coordinate = vec2f(clip.x, -clip.y)/max(abs(clip.w), 0.000001);
  let pixel = floor(((coordinate*0.5) + 0.5)*frame.viewport.xy);
  return fract((52.9829189*fract(dot(pixel, vec2f(0.06711056, 0.00583715)))) + frame.debug.y);
}

/**
 * A unit receiver normal, or straight up where a surface reports none.
 */
fn safeShadowNormal(normal: vec3f) -> vec3f {
  let lengthSquared = dot(normal, normal);
  if(lengthSquared < 0.000001) {
    return vec3f(0.0, 1.0, 0.0);
  }
  return normal*inverseSqrt(lengthSquared);
}
