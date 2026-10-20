// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Volumetric light injection: each froxel's medium (exponential height fog) and the light it scatters toward the eye
// from the key light, the point lights listed for the froxel's light cluster, and the skylight, with the key light and
// shadowed point lights casting volumetric shadows (one filtered comparison each). Each frame samples a different depth
// inside every froxel and blends with the previous frame's volume, reprojected, so the volume converges to a smooth
// average. Composed after constants.wgsl, scene-common.wgsl and shadow-receiver.wgsl (frame uniforms and light
// clusters in group 0, the shadow receiver in group 1), and volumetric-common.wgsl.

@group(2) @binding(0) var<uniform> volumetric: Volumetric;
@group(2) @binding(1) var historyVolume: texture_3d<f32>;
@group(2) @binding(2) var historySampler: sampler;
@group(2) @binding(3) var scatterVolume: texture_storage_3d<rgba16float, write>;

/**
 * Henyey-Greenstein phase function for the cosine between the light's travel and the view ray.
 */
fn henyeyGreenstein(cosine: f32, anisotropy: f32) -> f32 {
  let squared = anisotropy*anisotropy;
  let denominator = max(1.0 + squared - (2.0*anisotropy*cosine), 0.0001);
  return (1.0 - squared)/(4.0*PI*denominator*sqrt(denominator));
}

/**
 * The froxel's center in the world at a slice offset in [0, 1) within it.
 */
fn froxelWorldPosition(cell: vec3u, sliceOffset: f32) -> vec3f {
  let grid = volumetric.grid.xyz;
  let coordinate = (vec2f(cell.xy) + vec2f(0.5))/grid.xy;
  let depth = sliceDepth((f32(cell.z) + sliceOffset)/grid.z, volumetric.grid.w);
  let view = viewPositionAt(volumetric.inverseProjection, coordinate, depth);
  return (volumetric.inverseView*vec4f(view, 1.0)).xyz;
}

@compute @workgroup_size(8, 8, 1)
fn injectMain(@builtin(global_invocation_id) id: vec3u) {
  let grid = vec3u(volumetric.grid.xyz);
  if(any(id >= grid)) {
    return;
  }

  // The medium at a jittered depth inside the froxel.
  let worldPosition = froxelWorldPosition(id, volumetric.emissive.w);
  let heightAboveBase = worldPosition.y - volumetric.medium.z;
  let densityScale = exp(min(-volumetric.medium.y*heightAboveBase, 16.0));
  let density = volumetric.medium.x*densityScale;
  let viewRay = normalize(worldPosition - frame.eye.xyz);
  let castsShadows = volumetric.intensity.w > 0.5;

  // Key light.
  var light = vec3f(0.0);
  if(frame.primaryDirection.w > 0.0) {
    let direction = normalize(frame.primaryDirection.xyz);
    var visibility = 1.0;
    if(castsShadows) {
      visibility = directionalShadowSharp(worldPosition);
    }
    let phase = henyeyGreenstein(dot(direction, viewRay), volumetric.medium.w);
    light+= frame.primaryColor.rgb*frame.primaryDirection.w*phase*visibility*volumetric.intensity.x;
  }

  // The cluster's point lights, fading to zero at their range as on surfaces.
  let cluster = lightCluster(worldPosition);
  for(var entry = cluster.x; entry < cluster.x + cluster.y; entry++) {
    let point = pointLight(clusterLightIndex(entry));
    let reach = pointLightReach(point, worldPosition);
    if(reach <= 0.0) {
      continue;
    }
    var visibility = 1.0;
    if(castsShadows) {
      visibility = pointShadowSharp(i32(point.color.a) - 1, worldPosition);
    }
    let offset = point.positionRange.xyz - worldPosition;
    let phase = henyeyGreenstein(dot(offset/max(length(offset), 0.0001), viewRay), volumetric.medium.w);
    light+= point.color.rgb*reach*phase*visibility*volumetric.intensity.y;
  }

  // Skylight and ambient light arrive from every direction; as irradiance over pi they are the average radiance.
  let sky = 0.5*(frame.skylight.rgb + frame.groundSkylight.rgb)*frame.skylight.a;
  light+= ((sky + vec3f(frame.lighting.y))/PI)*volumetric.intensity.z;

  let scattering = (density*volumetric.albedo.rgb*light) + (volumetric.emissive.rgb*densityScale);
  var current = vec4f(scattering, density);

  // Blend with the previous frame's volume where this froxel's center was inside it.
  if(volumetric.albedo.w > 0.0) {
    let center = froxelWorldPosition(id, 0.5);
    let clip = volumetric.previousViewProjection*vec4f(center, 1.0);
    let previousDepth = -(volumetric.previousView*vec4f(center, 1.0)).z;
    if((clip.w > 0.0) && (previousDepth > 0.0)) {
      let coordinate = vec2f((clip.x/clip.w*0.5) + 0.5, 0.5 - (clip.y/clip.w*0.5));
      let slice = sqrt(previousDepth/volumetric.grid.w);
      let history = vec3f(coordinate, slice);
      if(all(history >= vec3f(0.0)) && all(history <= vec3f(1.0))) {
        let previous = textureSampleLevel(historyVolume, historySampler, history, 0.0);
        current = mix(current, previous, volumetric.albedo.w);
      }
    }
  }
  textureStore(scatterVolume, id, current);
}
