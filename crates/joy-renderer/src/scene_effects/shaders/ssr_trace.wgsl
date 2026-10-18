// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/screen-space-reflections.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

const SSR_TWO_PI: f32 = 6.283185307179586;

struct SSRTraceUniforms {
  projectionMatrix: mat4x4f,
  inverseProjectionMatrix: mat4x4f,
  intensity: f32,
  maxDistance: f32,
  thickness: f32,
  sampleCount: f32,
  maxRoughness: f32,
  frameIndex: f32,
};

@group(0) @binding(auto) var<uniform> ssrTrace: SSRTraceUniforms;
@group(0) @binding(auto) var depthTexture: texture_depth_2d;
@group(0) @binding(auto) var depthTextureSampler: sampler;
@group(0) @binding(auto) var normalTexture: texture_2d<f32>;
@group(0) @binding(auto) var normalTextureSampler: sampler;

fn ssrTrace_reconstructViewPosition(uv: vec2f, depth: f32) -> vec3f {
  let clip = vec4f(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
  let viewPosition = ssrTrace.inverseProjectionMatrix * clip;
  return viewPosition.xyz / max(viewPosition.w, 0.00001);
}

fn ssrTrace_projectViewPosition(position: vec3f) -> vec2f {
  let clip = ssrTrace.projectionMatrix * vec4f(position, 1.0);
  let normalizedDeviceCoordinate = clip.xy / max(clip.w, 0.00001);
  return vec2f(
    normalizedDeviceCoordinate.x * 0.5 + 0.5,
    0.5 - normalizedDeviceCoordinate.y * 0.5
  );
}

fn ssrTrace_hash(value: vec2f, frameIndex: f32) -> f32 {
  let temporalPhase = fract(frameIndex * 0.61803398875) * SSR_TWO_PI;
  return fract(sin(dot(value, vec2f(12.9898, 78.233)) + temporalPhase) * 43758.5453);
}

fn ssrTrace_sampleColor(
  sourceTexture: texture_2d<f32>,
  sourceTextureSampler: sampler,
  texSize: vec2f,
  texCoord: vec2f
) -> vec4f {
  let sceneCoord = texCoord;
  let normalRoughness = textureSampleLevel(normalTexture, normalTextureSampler, sceneCoord, 0);
  let normal = normalize(normalRoughness.xyz * 2.0 - 1.0);
  let roughness = clamp(normalRoughness.a, 0.0, 1.0);
  let centerDepth = textureSampleLevel(depthTexture, depthTextureSampler, sceneCoord, 0);
  if (centerDepth >= 0.99999 || roughness >= ssrTrace.maxRoughness ||
      ssrTrace.intensity <= 0.0001) {
    return vec4f(0.0);
  }

  let viewPosition = ssrTrace_reconstructViewPosition(sceneCoord, centerDepth);
  let incidentDirection = normalize(viewPosition);
  let mirrorDirection = normalize(reflect(incidentDirection, normal));
  let pixelCoordinate = floor(sceneCoord * vec2f(textureDimensions(depthTexture)));
  let noise = ssrTrace_hash(pixelCoordinate, ssrTrace.frameIndex);
  let noiseAngle = noise * SSR_TWO_PI;
  let referenceAxis = select(
    vec3f(0.0, 1.0, 0.0),
    vec3f(1.0, 0.0, 0.0),
    abs(mirrorDirection.y) > 0.9
  );
  let tangent = normalize(cross(referenceAxis, mirrorDirection));
  let bitangent = normalize(cross(mirrorDirection, tangent));
  let roughnessCone = roughness * roughness * 0.32;
  let reflectedRay = normalize(
    mirrorDirection + (tangent * cos(noiseAngle) + bitangent * sin(noiseAngle)) * roughnessCone
  );
  let rayOrigin = viewPosition + normal * max(ssrTrace.thickness * 0.12, 0.025);

  var reflection = vec3f(0.0);
  var confidence = 0.0;
  var previousTravel = 0.06;
  var previousDepthDelta = -ssrTrace.thickness;
  let minimumSampleCount = ceil(
    ssrTrace.maxDistance / max(ssrTrace.thickness * 0.72, 0.12)
  );
  let effectiveSampleCount = min(96.0, max(ssrTrace.sampleCount, minimumSampleCount));
  for (var sampleIndex: i32 = 1; sampleIndex <= 96; sampleIndex++) {
    if (f32(sampleIndex) > effectiveSampleCount) {
      break;
    }
    let fraction = (f32(sampleIndex) - noise * 0.42) / max(effectiveSampleCount, 1.0);
    let travel = 0.08 + pow(max(fraction, 0.0), 1.3) * ssrTrace.maxDistance;
    let rayPosition = rayOrigin + reflectedRay * travel;
    if (rayPosition.z >= -0.04) {
      break;
    }
    let sampleCoord = ssrTrace_projectViewPosition(rayPosition);
    if (any(sampleCoord <= vec2f(0.0)) || any(sampleCoord >= vec2f(1.0))) {
      break;
    }
    let sampleDepth = textureSampleLevel(depthTexture, depthTextureSampler, sampleCoord, 0);
    if (sampleDepth >= 0.99999) {
      previousTravel = travel;
      previousDepthDelta = -ssrTrace.thickness;
      continue;
    }
    let scenePosition = ssrTrace_reconstructViewPosition(sampleCoord, sampleDepth);
    let depthDelta = (-rayPosition.z) - (-scenePosition.z);
    let rayStepLength = max(travel - previousTravel, 0.001);
    let hitThickness = max(ssrTrace.thickness, rayStepLength * 1.2) + travel * 0.008;
    let crossesCandidateSurface = previousDepthDelta <= 0.0 && depthDelta >= 0.0;
    let screenTravelPixels = length((sampleCoord - sceneCoord) * texSize);
    var entersCandidateSurface = true;
    if (crossesCandidateSurface && depthDelta < hitThickness && screenTravelPixels > 1.25) {
      let candidateNormal = normalize(
        textureSampleLevel(normalTexture, normalTextureSampler, sampleCoord, 0).rgb * 2.0 - 1.0
      );
      entersCandidateSurface = dot(reflectedRay, candidateNormal) < -0.015;
      if (entersCandidateSurface) {
        var nearTravel = previousTravel;
        var farTravel = travel;
        var hitCoord = sampleCoord;
        for (var refinementIndex: i32 = 0; refinementIndex < 5; refinementIndex++) {
          let refinedTravel = (nearTravel + farTravel) * 0.5;
          let refinedPosition = rayOrigin + reflectedRay * refinedTravel;
          let refinedCoord = ssrTrace_projectViewPosition(refinedPosition);
          let refinedDepth = textureSampleLevel(depthTexture, depthTextureSampler, refinedCoord, 0);
          let refinedScenePosition = ssrTrace_reconstructViewPosition(refinedCoord, refinedDepth);
          if ((-refinedPosition.z) - (-refinedScenePosition.z) >= 0.0) {
            farTravel = refinedTravel;
            hitCoord = refinedCoord;
          } else {
            nearTravel = refinedTravel;
          }
        }
        reflection = textureSampleLevel(sourceTexture, sourceTextureSampler, hitCoord, 0).rgb;
        let screenEdge = min(
          min(hitCoord.x, hitCoord.y),
          min(1.0 - hitCoord.x, 1.0 - hitCoord.y)
        );
        let fresnel = mix(
          0.32,
          1.0,
          pow(1.0 - max(dot(-incidentDirection, normal), 0.0), 5.0)
        );
        let roughnessFade = pow(1.0 - roughness / max(ssrTrace.maxRoughness, 0.001), 1.5);
        let distanceFade = 1.0 - clamp(farTravel / ssrTrace.maxDistance, 0.0, 1.0);
        confidence = smoothstep(0.0, 0.09, screenEdge) * roughnessFade * fresnel *
          distanceFade * ssrTrace.intensity;
        break;
      }
    }
    previousTravel = travel;
    previousDepthDelta = select(-ssrTrace.thickness, depthDelta, entersCandidateSurface);
  }

  return vec4f(reflection, clamp(confidence, 0.0, 1.0));
}
