// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// The engine's direct light loop over a shading model's response, shared by forward shading (surface-scene.wgsl) and
// the deferred lighting passes (deferred-lighting.wgsl). Composed after the model, whose `materialDirect` it calls.

/**
 * Light arriving directly from the primary light (with its cascaded shadows) and from the point lights listed for the
 * surface's cluster (fading to zero at their range, with their shadows). Shadows are sampled along `normal`, the
 * model's shading normal, so receivers offset the way they shade.
 */
fn directLighting(surface: MaterialSurface, worldPosition: vec3f, normal: vec3f, receivesShadow: bool) -> vec3f {
  var radiance = vec3f(0.0);

  // Primary directional light.
  if(frame.primaryDirection.w > 0.0) {
    var visibility = 1.0;
    if(receivesShadow) {
      visibility = directionalShadow(worldPosition, normal);
    }
    var light: LightSample;
    light.direction = normalize(frame.primaryDirection.xyz);
    light.radiance = frame.primaryColor.rgb*frame.primaryDirection.w*visibility;
    radiance+= materialDirect(surface, light);
  }

  // Point lights in the surface's cluster.
  let cluster = lightCluster(worldPosition);
  for(var entry = cluster.x; entry < cluster.x + cluster.y; entry++) {
    let point = pointLight(clusterLightIndex(entry));
    let reach = pointLightReach(point, worldPosition);
    if(reach <= 0.0) {
      continue;
    }
    var visibility = 1.0;
    if(receivesShadow) {
      visibility = pointShadow(i32(point.color.a) - 1, worldPosition, normal);
    }
    let offset = point.positionRange.xyz - worldPosition;
    var light: LightSample;
    light.direction = offset/max(length(offset), 0.000001);
    light.radiance = point.color.rgb*reach*visibility;
    radiance+= materialDirect(surface, light);
  }
  return radiance;
}
