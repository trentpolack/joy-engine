// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/clustered-volumetric-lighting.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

const CLUSTERED_VOLUMETRIC_PI: f32 = 3.141592653589793;
const CLUSTERED_VOLUMETRIC_MAX_STEPS: u32 = 20u;
const CLUSTERED_VOLUMETRIC_MAX_LIGHTS: u32 = 10u;
const CLUSTERED_VOLUMETRIC_MAX_GOD_RAY_STEPS: u32 = 32u;

struct ClusteredVolumetricTraceUniforms {
  projectionMatrix: mat4x4f,
  inverseProjectionMatrix: mat4x4f,
  inverseViewMatrix: mat4x4f,
  directionalLightDirectionView: vec3f,
  directionalLightColor: vec3f,
  fogColor: vec3f,
  godRayPosition: vec2f,
  density: f32,
  heightFalloff: f32,
  fogHeight: f32,
  anisotropy: f32,
  directionalIntensity: f32,
  pointLightIntensity: f32,
  godRayIntensity: f32,
  godRayDensity: f32,
  godRayDecay: f32,
  godRaySampleCount: f32,
  godRaysOnly: f32,
  maxDistance: f32,
  sampleCount: f32,
  shadowStrength: f32,
  clusterCountX: u32,
  clusterCountY: u32,
  clusterCountZ: u32,
  maxLightsPerCluster: u32,
  pointLightCount: u32,
  clusterNearPlane: f32,
  clusterFarPlane: f32,
};

struct ClusteredVolumetricPointLight {
  positionRange: vec4f,
  colorIntensity: vec4f,
};

@group(0) @binding(auto) var<uniform> clusteredVolumetricTrace:
  ClusteredVolumetricTraceUniforms;
@group(0) @binding(auto) var depthTexture: texture_depth_2d;
@group(0) @binding(auto) var depthTextureSampler: sampler;
@group(0) @binding(auto) var<storage, read> pointLights:
  array<ClusteredVolumetricPointLight>;
@group(0) @binding(auto) var<storage, read> clusterLightCounts: array<u32>;
@group(0) @binding(auto) var<storage, read> clusterLightIndices: array<u32>;

fn clusteredVolumetricTrace_reconstructViewPosition(texCoord: vec2f, depth: f32) -> vec3f {
  let clip = vec4f(texCoord.x * 2.0 - 1.0, 1.0 - texCoord.y * 2.0, depth, 1.0);
  let viewPosition = clusteredVolumetricTrace.inverseProjectionMatrix * clip;
  return viewPosition.xyz / max(abs(viewPosition.w), 0.00001);
}

fn clusteredVolumetricTrace_projectViewPosition(viewPosition: vec3f) -> vec2f {
  let clip = clusteredVolumetricTrace.projectionMatrix * vec4f(viewPosition, 1.0);
  let normalizedDeviceCoordinate = clip.xy / max(clip.w, 0.00001);
  return normalizedDeviceCoordinate * vec2f(0.5, -0.5) + vec2f(0.5);
}

fn clusteredVolumetricTrace_phase(cosine: f32, anisotropy: f32) -> f32 {
  let squaredAnisotropy = anisotropy * anisotropy;
  let denominator = max(1.0 + squaredAnisotropy - 2.0 * anisotropy * cosine, 0.02);
  return (1.0 - squaredAnisotropy) /
    (4.0 * CLUSTERED_VOLUMETRIC_PI * pow(denominator, 1.5));
}

fn clusteredVolumetricTrace_clusterIndex(texCoord: vec2f, viewDepth: f32) -> u32 {
  let tileX = min(
    u32(clamp(texCoord.x, 0.0, 0.999999) * f32(clusteredVolumetricTrace.clusterCountX)),
    clusteredVolumetricTrace.clusterCountX - 1u
  );
  let tileY = min(
    u32(clamp(texCoord.y, 0.0, 0.999999) * f32(clusteredVolumetricTrace.clusterCountY)),
    clusteredVolumetricTrace.clusterCountY - 1u
  );
  let normalizedDepth = clamp(
    log(max(viewDepth, clusteredVolumetricTrace.clusterNearPlane) /
      clusteredVolumetricTrace.clusterNearPlane) /
      log(clusteredVolumetricTrace.clusterFarPlane /
        clusteredVolumetricTrace.clusterNearPlane),
    0.0,
    0.999999
  );
  let tileZ = min(
    u32(normalizedDepth * f32(clusteredVolumetricTrace.clusterCountZ)),
    clusteredVolumetricTrace.clusterCountZ - 1u
  );
  return tileX + clusteredVolumetricTrace.clusterCountX *
    (tileY + clusteredVolumetricTrace.clusterCountY * tileZ);
}

fn clusteredVolumetricTrace_directionalVisibility(viewPosition: vec3f) -> f32 {
  let lightDirection = normalize(clusteredVolumetricTrace.directionalLightDirectionView);
  for (var shadowStep: u32 = 1u; shadowStep <= 3u; shadowStep++) {
    let shadowPosition = viewPosition + lightDirection * (0.32 + f32(shadowStep) * 0.95);
    if (shadowPosition.z >= -0.04) {
      return 1.0;
    }
    let shadowCoord = clusteredVolumetricTrace_projectViewPosition(shadowPosition);
    if (any(shadowCoord <= vec2f(0.0)) || any(shadowCoord >= vec2f(1.0))) {
      return 1.0;
    }
    let occluderDepth = textureSampleLevel(depthTexture, depthTextureSampler, shadowCoord, 0);
    if (occluderDepth >= 0.99999) {
      continue;
    }
    let occluderPosition = clusteredVolumetricTrace_reconstructViewPosition(
      shadowCoord,
      occluderDepth
    );
    let depthBehindOccluder = -shadowPosition.z - (-occluderPosition.z);
    if (depthBehindOccluder > 0.06 && depthBehindOccluder < 1.9) {
      return 1.0 - clusteredVolumetricTrace.shadowStrength;
    }
  }
  return 1.0;
}

fn clusteredVolumetricTrace_godRayVisibility(texCoord: vec2f) -> f32 {
  if (clusteredVolumetricTrace.godRayIntensity <= 0.0001) {
    return 0.0;
  }

  let sampleCount = clamp(
    u32(clusteredVolumetricTrace.godRaySampleCount),
    3u,
    CLUSTERED_VOLUMETRIC_MAX_GOD_RAY_STEPS
  );
  let sampleOffset = (texCoord - clusteredVolumetricTrace.godRayPosition) *
    clusteredVolumetricTrace.godRayDensity / f32(sampleCount);
  var sampleCoord = texCoord;
  var illumination = 0.0;
  var totalWeight = 0.0;
  var illuminationDecay = 1.0;

  for (var sampleIndex: u32 = 0u;
      sampleIndex < CLUSTERED_VOLUMETRIC_MAX_GOD_RAY_STEPS;
      sampleIndex++) {
    if (sampleIndex >= sampleCount) {
      break;
    }
    sampleCoord -= sampleOffset;
    if (any(sampleCoord < vec2f(0.0)) || any(sampleCoord > vec2f(1.0))) {
      break;
    }
    let occluderDepth = textureSampleLevel(depthTexture, depthTextureSampler, sampleCoord, 0);
    let visibility = smoothstep(0.9985, 0.99999, occluderDepth);
    illumination += visibility * illuminationDecay;
    totalWeight += illuminationDecay;
    illuminationDecay *= clusteredVolumetricTrace.godRayDecay;
  }

  let distanceFromSun = distance(texCoord, clusteredVolumetricTrace.godRayPosition);
  return illumination / max(totalWeight, 0.0001) * exp(-distanceFromSun * 1.6);
}

fn clusteredVolumetricTrace_pointRadiance(
  texCoord: vec2f,
  viewPosition: vec3f,
  viewDirection: vec3f
) -> vec3f {
  let clusterIndex = clusteredVolumetricTrace_clusterIndex(texCoord, -viewPosition.z);
  let clusterCount = clusterLightCounts[clusterIndex];
  let firstCandidateIndex = clusterIndex * clusteredVolumetricTrace.maxLightsPerCluster;
  let clampedFirstCandidateIndex = min(firstCandidateIndex, arrayLength(&clusterLightIndices));
  let storedCandidateCapacity = min(
    clusteredVolumetricTrace.maxLightsPerCluster,
    arrayLength(&clusterLightIndices) - clampedFirstCandidateIndex
  );
  let candidateCount = min(clusterCount, storedCandidateCapacity);
  let activePointLightCount = min(
    clusteredVolumetricTrace.pointLightCount,
    arrayLength(&pointLights)
  );
  var selectedLightIndices: array<u32, CLUSTERED_VOLUMETRIC_MAX_LIGHTS>;
  var selectedLightScores: array<f32, CLUSTERED_VOLUMETRIC_MAX_LIGHTS>;
  for (var slotIndex: u32 = 0u; slotIndex < CLUSTERED_VOLUMETRIC_MAX_LIGHTS; slotIndex++) {
    selectedLightIndices[slotIndex] = 0xffffffffu;
    selectedLightScores[slotIndex] = 1.0e20;
  }
  var selectedLightCount = 0u;
  var worstSlotIndex = 0u;
  var worstScore = -1.0;

  // Score every candidate retained by the cluster builder, including its complete overflow-safe
  // storage capacity, while keeping integration work independent of the global light count.
  for (var candidateIndex: u32 = 0u; candidateIndex < candidateCount; candidateIndex++) {
    let lightIndex = clusterLightIndices[firstCandidateIndex + candidateIndex];
    if (lightIndex >= activePointLightCount) {
      continue;
    }
    let light = pointLights[lightIndex];
    let toLight = light.positionRange.xyz - viewPosition;
    let distanceSquared = dot(toLight, toLight);
    let rangeSquared = light.positionRange.w * light.positionRange.w;
    if (distanceSquared >= rangeSquared || distanceSquared <= 0.0004) {
      continue;
    }
    let lightScore = distanceSquared /
      max(rangeSquared * max(light.colorIntensity.a, 0.05), 0.0001);
    if (selectedLightCount < CLUSTERED_VOLUMETRIC_MAX_LIGHTS) {
      selectedLightIndices[selectedLightCount] = lightIndex;
      selectedLightScores[selectedLightCount] = lightScore;
      if (lightScore > worstScore) {
        worstSlotIndex = selectedLightCount;
        worstScore = lightScore;
      }
      selectedLightCount += 1u;
      continue;
    }
    if (lightScore < worstScore) {
      selectedLightIndices[worstSlotIndex] = lightIndex;
      selectedLightScores[worstSlotIndex] = lightScore;
      worstSlotIndex = 0u;
      worstScore = selectedLightScores[0];
      for (var slotIndex = 1u; slotIndex < CLUSTERED_VOLUMETRIC_MAX_LIGHTS; slotIndex++) {
        if (selectedLightScores[slotIndex] > worstScore) {
          worstSlotIndex = slotIndex;
          worstScore = selectedLightScores[slotIndex];
        }
      }
    }
  }

  var radiance = vec3f(0.0);
  for (var slotIndex: u32 = 0u; slotIndex < CLUSTERED_VOLUMETRIC_MAX_LIGHTS; slotIndex++) {
    let lightIndex = selectedLightIndices[slotIndex];
    if (lightIndex >= arrayLength(&pointLights)) {
      continue;
    }
    let light = pointLights[lightIndex];
    let toLight = light.positionRange.xyz - viewPosition;
    let distanceToLight = length(toLight);
    if (distanceToLight >= light.positionRange.w || distanceToLight <= 0.02) {
      continue;
    }
    let lightDirection = toLight / distanceToLight;
    let rangeFade = pow(1.0 - distanceToLight / light.positionRange.w, 2.0);
    let attenuation = rangeFade / (1.0 + distanceToLight * distanceToLight * 0.1);
    let phase = clusteredVolumetricTrace_phase(
      dot(viewDirection, lightDirection),
      clusteredVolumetricTrace.anisotropy * 0.65
    );
    radiance += light.colorIntensity.rgb * light.colorIntensity.a *
      attenuation * phase * clusteredVolumetricTrace.pointLightIntensity;
  }
  return radiance;
}

fn clusteredVolumetricTrace_sampleColor(
  sourceTexture: texture_2d<f32>,
  sourceTextureSampler: sampler,
  texSize: vec2f,
  texCoord: vec2f
) -> vec4f {
  if (clusteredVolumetricTrace.density <= 0.0001) {
    return vec4f(0.0, 0.0, 0.0, 1.0);
  }
  let sceneDepth = textureSampleLevel(depthTexture, depthTextureSampler, texCoord, 0);
  let rayOrigin = clusteredVolumetricTrace_reconstructViewPosition(texCoord, 0.0);
  let farPosition = clusteredVolumetricTrace_reconstructViewPosition(texCoord, 0.9999);
  let viewDirection = normalize(farPosition - rayOrigin);
  let scenePosition = clusteredVolumetricTrace_reconstructViewPosition(texCoord, sceneDepth);
  let rayDistance = select(
    min(
      max(dot(scenePosition - rayOrigin, viewDirection), 0.0),
      clusteredVolumetricTrace.maxDistance
    ),
    clusteredVolumetricTrace.maxDistance,
    sceneDepth >= 0.99999
  );
  let stepCount = clamp(u32(clusteredVolumetricTrace.sampleCount), 3u,
    CLUSTERED_VOLUMETRIC_MAX_STEPS);
  let stepDistance = rayDistance / f32(stepCount);
  let pixelCoordinate = floor(texCoord * texSize);
  let rayJitter = fract(sin(dot(pixelCoordinate, vec2f(12.9898, 78.233))) * 43758.5453);
  let directionalPhase = clusteredVolumetricTrace_phase(
    dot(viewDirection, normalize(clusteredVolumetricTrace.directionalLightDirectionView)),
    clusteredVolumetricTrace.anisotropy
  );
  let godRayVisibility = clusteredVolumetricTrace_godRayVisibility(texCoord);

  var opticalDepth = 0.0;
  var scattering = vec3f(0.0);
  for (var stepIndex: u32 = 0u; stepIndex < CLUSTERED_VOLUMETRIC_MAX_STEPS; stepIndex++) {
    if (stepIndex >= stepCount) {
      break;
    }
    let travel = (f32(stepIndex) + 0.28 + rayJitter * 0.52) * stepDistance;
    let viewPosition = rayOrigin + viewDirection * travel;
    let worldPosition =
      (clusteredVolumetricTrace.inverseViewMatrix * vec4f(viewPosition, 1.0)).xyz;
    let localDensity = clusteredVolumetricTrace.density * exp(
      -max(worldPosition.y - clusteredVolumetricTrace.fogHeight, 0.0) *
        clusteredVolumetricTrace.heightFalloff
    );
    let extinction = localDensity * stepDistance;
    opticalDepth += extinction;
    let directionalVisibility = clusteredVolumetricTrace_directionalVisibility(viewPosition);
    let directionalRadiance = clusteredVolumetricTrace.directionalLightColor *
      clusteredVolumetricTrace.directionalIntensity * directionalPhase * directionalVisibility;
    let godRayRadiance = clusteredVolumetricTrace.directionalLightColor *
      clusteredVolumetricTrace.directionalIntensity * directionalPhase * godRayVisibility *
      clusteredVolumetricTrace.godRayIntensity * 2.4;
    let localRadiance = clusteredVolumetricTrace_pointRadiance(
      texCoord,
      viewPosition,
      viewDirection
    );
    let incomingRadiance = select(
      clusteredVolumetricTrace.fogColor * 0.11 + directionalRadiance + localRadiance +
        godRayRadiance,
      godRayRadiance,
      clusteredVolumetricTrace.godRaysOnly > 0.5
    );
    scattering += incomingRadiance * extinction * exp(-opticalDepth);
  }

  return vec4f(scattering, clamp(exp(-opticalDepth), 0.0, 1.0));
}
