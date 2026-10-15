// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// PBR material: world-space GGX metallic/roughness shading driven by per-vertex material factors (FORM materials are
// baked into the vertex stream at upload). Matches the TypeScript PBR material pass without texture maps or image-based
// lighting, which remain follow-up work. Prepended at load time by scene-common.wgsl.

struct PbrOutput {
  @builtin(position) position: vec4f,
  @location(0) worldPosition: vec3f,
  @location(1) worldNormal: vec3f,
  @location(2) color: vec4f,
  @location(3) surface: vec4f,
  @location(4) emissive: vec3f,
  @location(5) receivesShadow: f32,
};

/**
 * Transform an instanced vertex and prepare world-space material inputs.
 * The cofactor normal transform keeps lighting correct under non-uniform scale.
 */
@vertex fn vertexMain(vertex: MeshVertex, instance: Instance) -> PbrOutput {
  let model = instanceModel(instance);
  let world = model*vec4f(vertex.position, 1.0);

  var output: PbrOutput;
  output.position = frame.viewProjection*world;
  output.worldPosition = world.xyz;
  output.worldNormal = worldNormal(model, vertex.normal);
  output.color = vertex.color*instance.tint;
  output.surface = vertex.surface;
  output.emissive = vertex.emissive.rgb;
  output.receivesShadow = instance.flags.x;
  return output;
}

/**
 * Shade one fragment with the primary light, point lights, ambient skylight, and emission.
 * Zero-length authored normals fall back to the faceted derivative normal, so flat FORM geometry still lights.
 */
@fragment fn fragmentMain(input: PbrOutput) -> @location(0) vec4f {
  let derivative = cross(dpdx(input.worldPosition), dpdy(input.worldPosition));
  var normal = input.worldNormal;
  if(dot(normal, normal) < 0.000001) {
    normal = derivative/max(length(derivative), 0.000001);
  }
  let n = faceEye(normalize(normal), input.worldPosition);

  // Alpha modes: opaque ignores alpha, mask discards below the cutoff, blend keeps straight alpha.
  var alpha = input.color.a;
  if(input.surface.w < 0.5) {
    alpha = 1.0;
  } else if(input.surface.w < 1.5) {
    if(alpha < input.surface.z) {
      discard;
    }
    alpha = 1.0;
  }

  let base = input.color.rgb;
  let metallic = clamp(input.surface.x, 0.0, 1.0);
  let roughness = clamp(input.surface.y, 0.045, 1.0);
  let v = normalize(frame.eye.xyz - input.worldPosition);

  var visibility = 1.0;
  if(input.receivesShadow > 0.5) {
    visibility = directionalShadow(input.worldPosition, n);
  }
  let primaryRadiance = frame.primaryColor.rgb*frame.primaryDirection.w;
  var radiance = pbrDirect(base, metallic, roughness, n, v, normalize(frame.primaryDirection.xyz))*primaryRadiance*visibility;

  let lightCount = min(u32(frame.lighting.x), MAX_POINT_LIGHTS);
  for(var index = 0u; index < lightCount; index++) {
    let light = frame.lights[index];
    let offset = light.positionRange.xyz - input.worldPosition;
    let distance = length(offset);
    let direction = offset/max(distance, 0.000001);
    let reach = max(0.0, 1.0 - (distance/light.positionRange.w));
    radiance+= pbrDirect(base, metallic, roughness, n, v, direction)*light.color.rgb*(reach*reach);
  }

  let ambient = vec3f(frame.lighting.y) + (frame.skylight.rgb*frame.skylight.a);
  radiance+= (ambient*base*(1.0 - metallic)) + input.emissive;
  return vec4f(radiance, alpha);
}
