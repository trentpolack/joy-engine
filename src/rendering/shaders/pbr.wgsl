// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// PBR material: world-space GGX metallic/roughness shading. Factors are baked into each vertex (FORM materials and
// `PbrMesh` palettes); texture maps multiply them, normal maps perturb the surface through a derivative tangent frame,
// and an optional environment adds image-based diffuse and specular light. Matches the TypeScript PBR material pass.
// Prepended at load time by scene-common.wgsl.

struct ImageLighting {
  // X intensity (zero disables image lighting), Y horizontal rotation in radians, Z highest specular mip.
  parameters: vec4f,
};

struct Material {
  // X has a normal map (0 or 1).
  flags: vec4f,
};

@group(2) @binding(0) var<uniform> imageLighting: ImageLighting;
@group(2) @binding(1) var diffuseEnvironment: texture_cube<f32>;
@group(2) @binding(2) var specularEnvironment: texture_cube<f32>;
@group(2) @binding(3) var brdfTable: texture_2d<f32>;
@group(2) @binding(4) var environmentSampler: sampler;

@group(3) @binding(0) var<uniform> material: Material;
@group(3) @binding(1) var baseColorMap: texture_2d<f32>;
@group(3) @binding(2) var normalMap: texture_2d<f32>;
@group(3) @binding(3) var emissiveMap: texture_2d<f32>;
@group(3) @binding(4) var metallicMap: texture_2d<f32>;
@group(3) @binding(5) var roughnessMap: texture_2d<f32>;
@group(3) @binding(6) var occlusionMap: texture_2d<f32>;
@group(3) @binding(7) var materialSampler: sampler;

struct PbrOutput {
  @builtin(position) position: vec4f,
  @location(0) worldPosition: vec3f,
  @location(1) worldNormal: vec3f,
  @location(2) color: vec4f,
  @location(3) surface: vec4f,
  @location(4) emissive: vec3f,
  @location(5) receivesShadow: f32,
  @location(6) uv: vec2f,
};

fn linearColor(color: vec3f) -> vec3f {
  return select(color/12.92, pow((color + 0.055)/1.055, vec3f(2.4)), color >= vec3f(0.04045));
}

/**
 * Transform an instanced vertex and prepare world-space material inputs.
 * The cofactor normal transform keeps lighting correct under non-uniform scale.
 */
@vertex fn vertexMain(vertex: MeshVertex, instance: Instance, @location(15) uv: vec2f) -> PbrOutput {
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
  output.uv = uv;
  return output;
}

/**
 * Shade one fragment with the primary light, point lights, ambient skylight, image-based light, and emission.
 * Zero-length authored normals fall back to the faceted derivative normal, so flat FORM geometry still lights.
 */
@fragment fn fragmentMain(input: PbrOutput, @builtin(front_facing) frontFacing: bool) -> @location(0) vec4f {
  // Sample before alpha discard so derivative-based tangent frames remain valid.
  let sampledBase = textureSample(baseColorMap, materialSampler, input.uv);
  let sampledEmission = textureSample(emissiveMap, materialSampler, input.uv).rgb;
  let sampledMetallic = textureSample(metallicMap, materialSampler, input.uv).r;
  let sampledRoughness = textureSample(roughnessMap, materialSampler, input.uv).r;
  let occlusion = textureSample(occlusionMap, materialSampler, input.uv).r;
  let tangentNormal = (textureSample(normalMap, materialSampler, input.uv).xyz*2.0) - 1.0;
  let positionX = dpdx(input.worldPosition);
  let positionY = dpdy(input.worldPosition);
  let uvX = dpdx(input.uv);
  let uvY = dpdy(input.uv);

  // Double-sided like the TypeScript pass: back faces flip the authored normal, and front faces keep it even where
  // smooth shading tips it away from the eye, so silhouettes keep their grazing-angle response. Normals rebuilt from
  // screen-space derivatives have no authored side and face the eye instead.
  let derivative = cross(positionX, positionY);
  var n = normalize(input.worldNormal)*select(-1.0, 1.0, frontFacing);
  if(dot(input.worldNormal, input.worldNormal) < 0.000001) {
    n = faceEye(derivative/max(length(derivative), 0.000001), input.worldPosition);
  }
  let determinant = (uvX.x*uvY.y) - (uvX.y*uvY.x);
  var tangent = (positionX*uvY.y) - (positionY*uvX.y);
  tangent-= n*dot(n, tangent);
  // Degeneracy thresholds scale with the derivatives. The TypeScript pass compares against fixed 1e-7 values, which
  // screen-space derivatives at ordinary viewing distances fall below, so its normal maps rarely apply.
  let uvScale = max(dot(uvX, uvX), dot(uvY, uvY));
  let positionScale = max(dot(positionX, positionX), dot(positionY, positionY));
  let mapped = (abs(determinant) > (0.000001*uvScale)) && (dot(tangent, tangent) > (0.000001*uvScale*positionScale));
  if((material.flags.x > 0.5) && mapped) {
    tangent = normalize(tangent)*sign(determinant);
    let bitangent = normalize(cross(n, tangent))*sign(determinant);
    n = normalize((tangent*tangentNormal.x) + (bitangent*tangentNormal.y) + (n*tangentNormal.z));
  }

  // Alpha modes: opaque ignores alpha, mask discards below the cutoff, blend keeps straight alpha.
  var alpha = input.color.a*sampledBase.a;
  if(input.surface.w < 0.5) {
    alpha = 1.0;
  } else if(input.surface.w < 1.5) {
    if(alpha < input.surface.z) {
      discard;
    }
    alpha = 1.0;
  }

  let base = input.color.rgb*linearColor(sampledBase.rgb);
  let metallic = clamp(input.surface.x*sampledMetallic, 0.0, 1.0);
  let roughness = clamp(input.surface.y*sampledRoughness, 0.045, 1.0);
  let emission = input.emissive*linearColor(sampledEmission);
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
    var lightVisibility = 1.0;
    if(input.receivesShadow > 0.5) {
      lightVisibility = pointShadow(i32(light.color.a) - 1, input.worldPosition, n);
    }
    radiance+= pbrDirect(base, metallic, roughness, n, v, direction)*light.color.rgb*(reach*reach)*lightVisibility;
  }

  let ambient = vec3f(frame.lighting.y) + (frame.skylight.rgb*frame.skylight.a);
  radiance+= (ambient*base*(1.0 - metallic)*occlusion) + emission;

  let intensity = imageLighting.parameters.x;
  if(intensity > 0.0) {
    let angle = imageLighting.parameters.y;
    let cosine = cos(angle);
    let sine = sin(angle);
    let reflection = reflect(-v, n);
    let environmentNormal = vec3f((cosine*n.x) - (sine*n.z), n.y, (sine*n.x) + (cosine*n.z));
    let environmentReflection =
      vec3f((cosine*reflection.x) - (sine*reflection.z), reflection.y, (sine*reflection.x) + (cosine*reflection.z));
    let diffuseLight = textureSampleLevel(diffuseEnvironment, environmentSampler, environmentNormal, 0.0).rgb;
    let specularLight = textureSampleLevel(specularEnvironment, environmentSampler, environmentReflection,
      roughness*imageLighting.parameters.z).rgb;
    let nv = max(dot(n, v), 0.0001);
    let brdf = textureSampleLevel(brdfTable, environmentSampler, vec2f(nv, roughness), 0.0).rg;
    let f0 = mix(vec3f(0.04), base, metallic);
    let fresnel = f0 + ((max(vec3f(1.0 - roughness), f0) - f0)*pow(1.0 - nv, 5.0));
    let diffuse = (vec3f(1.0) - fresnel)*(1.0 - metallic)*base*diffuseLight;
    radiance+= (diffuse + (specularLight*((f0*brdf.x) + vec3f(brdf.y))))*occlusion*intensity;
  }
  return vec4f(radiance, alpha);
}
