// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Intersect each cube sample ray with the biased receiver plane. A center-only
// reference falsely identifies sloped planar receivers as blockers across a
// wide filter kernel, especially where the dominant cube axis changes.
fn scene_pointPlaneDepth(direction: vec3f, fromLight: vec3f, normal: vec3f, nearPlane: f32, farPlane: f32) -> f32 {
  let denominator = dot(normal, direction);
  if(abs(denominator) < 0.00001) {
    return 0.0;
  }
  let distance = dot(normal, fromLight)/denominator;
  let majorDistance = distance*max(max(abs(direction.x), abs(direction.y)), abs(direction.z));
  // Rays parallel to, behind, or before the receiver's light frustum cannot
  // provide a meaningful blocker comparison. Treat those taps as unoccluded.
  if(majorDistance <= nearPlane || majorDistance >= farPlane) {
    return 0.0;
  }
  return shadow_pointReferenceDepth(majorDistance, nearPlane, farPlane);
}

fn shadow_getPointFactor(lightIndex: i32, worldPosition: vec3f, worldNormal: vec3f) -> f32 {
  if(lightIndex < 0 || lightIndex >= shadow.pointLightCount) {
    return 1.0;
  }
  let light = shadow.pointLights[lightIndex];
  let normal = normalize(worldNormal);
  let fromLight = worldPosition + normal*light.normalBias - light.position;
  let distanceToLight = length(fromLight);
  if(distanceToLight >= light.range || distanceToLight <= light.nearPlane) {
    return 1.0;
  }
  let direction = fromLight/distanceToLight;
  let majorDistance = max(max(abs(fromLight.x), abs(fromLight.y)), abs(fromLight.z));
  let referenceDepth = shadow_pointReferenceDepth(majorDistance, light.nearPlane, light.range);
  let searchRadius = light.sourceRadius/max(distanceToLight, 0.001);
  let rotation = shadow_hash(worldPosition)*2.0*SHADOW_PI;
  let basis = shadow_pointBasis(direction);
  var blockerDepth = 0.0;
  var blockerCount = 0.0;
  for(var index = 0; index < 24; index++) {
    if(index >= shadow.blockerSampleCount) {
      break;
    }
    let disk = shadow_diskSample(index, shadow.blockerSampleCount, rotation)*searchRadius;
    let sampleDirection = normalize(direction + basis[0]*disk.x + basis[1]*disk.y);
    let receiverDepth = scene_pointPlaneDepth(sampleDirection, fromLight, normal, light.nearPlane, light.range);
    let sampleDepth = textureSampleLevel(pointShadowTexture, directionalShadowTextureSampler, sampleDirection, lightIndex, 0);
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
  let filterRadius = searchRadius*(1.0 + penumbra*12.0);
  var visibility = 0.0;
  for(var index = 0; index < 48; index++) {
    if(index >= shadow.filterSampleCount) {
      break;
    }
    let disk = shadow_diskSample(index, shadow.filterSampleCount, rotation)*filterRadius;
    let sampleDirection = normalize(direction + basis[0]*disk.x + basis[1]*disk.y);
    let receiverDepth = scene_pointPlaneDepth(sampleDirection, fromLight, normal, light.nearPlane, light.range);
    visibility+= textureSampleCompareLevel(pointShadowTexture, shadowComparisonSampler, sampleDirection, lightIndex, receiverDepth);
  }
  return mix(1.0, visibility/max(f32(shadow.filterSampleCount), 1.0), light.strength);
}
