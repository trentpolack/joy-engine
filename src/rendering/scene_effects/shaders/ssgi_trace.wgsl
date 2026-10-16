// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/screen-space-global-illumination.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

const SSGI_TWO_PI: f32 = 6.283185307179586;
const SSGI_GOLDEN_ANGLE: f32 = 2.399963229728653;
const SSGI_MAX_RAYS: i32 = 12;
const SSGI_MAX_STEPS: i32 = 12;

struct SSGITraceUniforms {
  projectionMatrix: mat4x4f,
  inverseProjectionMatrix: mat4x4f,
  radius: f32,
  thickness: f32,
  intensity: f32,
  rayCount: f32,
  stepCount: f32,
  frameIndex: f32,
};

@group(0) @binding(auto) var<uniform> ssgiTrace: SSGITraceUniforms;
@group(0) @binding(auto) var depthTexture: texture_depth_2d;
@group(0) @binding(auto) var depthTextureSampler: sampler;
@group(0) @binding(auto) var normalTexture: texture_2d<f32>;
@group(0) @binding(auto) var normalTextureSampler: sampler;

fn ssgiTrace_reconstructViewPosition(texCoord: vec2f, depth: f32) -> vec3f {
  let clip = vec4f(texCoord.x * 2.0 - 1.0, 1.0 - texCoord.y * 2.0, depth, 1.0);
  let viewPosition = ssgiTrace.inverseProjectionMatrix * clip;
  return viewPosition.xyz / max(viewPosition.w, 0.00001);
}

fn ssgiTrace_projectViewPosition(viewPosition: vec3f) -> vec2f {
  let clip = ssgiTrace.projectionMatrix * vec4f(viewPosition, 1.0);
  let normalizedDeviceCoordinate = clip.xy / max(clip.w, 0.00001);
  return normalizedDeviceCoordinate * vec2f(0.5, -0.5) + vec2f(0.5);
}

fn ssgiTrace_hash(value: vec2f) -> f32 {
  return fract(sin(dot(value, vec2f(12.9898, 78.233))) * 43758.5453);
}

fn ssgiTrace_sampleColor(
  sourceTexture: texture_2d<f32>,
  sourceTextureSampler: sampler,
  texSize: vec2f,
  texCoord: vec2f
) -> vec4f {
  let centerDepth = textureSampleLevel(depthTexture, depthTextureSampler, texCoord, 0);
  if (centerDepth >= 0.99999 || ssgiTrace.intensity <= 0.0001) {
    return vec4f(0.0);
  }

  let centerPosition = ssgiTrace_reconstructViewPosition(texCoord, centerDepth);
  let centerNormalSample = textureSampleLevel(normalTexture, normalTextureSampler, texCoord, 0);
  let centerNormal = normalize(centerNormalSample.rgb * 2.0 - 1.0);
  let roughness = clamp(centerNormalSample.a, 0.0, 1.0);
  let referenceAxis = select(
    vec3f(0.0, 1.0, 0.0),
    vec3f(1.0, 0.0, 0.0),
    abs(centerNormal.y) > 0.9
  );
  let tangent = normalize(cross(referenceAxis, centerNormal));
  let bitangent = normalize(cross(centerNormal, tangent));
  let pixelCoordinate = floor(texCoord * vec2f(textureDimensions(depthTexture)));
  let rotation = ssgiTrace_hash(pixelCoordinate) * SSGI_TWO_PI;
  let activeRayCount = clamp(i32(ssgiTrace.rayCount), 1, SSGI_MAX_RAYS);
  let activeStepCount = clamp(i32(ssgiTrace.stepCount), 2, SSGI_MAX_STEPS);

  var indirectRadiance = vec3f(0.0);
  var accumulatedWeight = 0.0;
  var hitCount = 0.0;
  for (var rayIndex: i32 = 0; rayIndex < SSGI_MAX_RAYS; rayIndex++) {
    if (rayIndex >= activeRayCount) {
      break;
    }

    let distribution = (f32(rayIndex) + 0.5) / f32(activeRayCount);
    let cosine = sqrt(1.0 - distribution * 0.82);
    let sine = sqrt(max(1.0 - cosine * cosine, 0.0));
    let angle = rotation + f32(rayIndex) * SSGI_GOLDEN_ANGLE;
    let rayDirection = normalize(
      centerNormal * cosine +
        (tangent * cos(angle) + bitangent * sin(angle)) * sine
    );
    let rayOrigin = centerPosition + centerNormal * max(ssgiTrace.thickness * 0.2, 0.025);
    var previousDepthDelta = -ssgiTrace.thickness;

    for (var stepIndex: i32 = 1; stepIndex <= SSGI_MAX_STEPS; stepIndex++) {
      if (stepIndex > activeStepCount) {
        break;
      }

      let stepFraction = f32(stepIndex) / f32(activeStepCount);
      let travel = max(ssgiTrace.radius * pow(stepFraction, 1.35), 0.04);
      let rayPosition = rayOrigin + rayDirection * travel;
      if (rayPosition.z >= -0.04) {
        break;
      }

      let sampleCoord = ssgiTrace_projectViewPosition(rayPosition);
      if (any(sampleCoord <= vec2f(0.0)) || any(sampleCoord >= vec2f(1.0))) {
        break;
      }

      let sampleDepth = textureSampleLevel(depthTexture, depthTextureSampler, sampleCoord, 0);
      if (sampleDepth >= 0.99999) {
        previousDepthDelta = -ssgiTrace.thickness;
        continue;
      }

      let samplePosition = ssgiTrace_reconstructViewPosition(sampleCoord, sampleDepth);
      let depthDelta = (-rayPosition.z) - (-samplePosition.z);
      let stepDistance = ssgiTrace.radius / f32(activeStepCount);
      let hitThickness = max(ssgiTrace.thickness, stepDistance * 1.25);
      let screenDistance = length((sampleCoord - texCoord) * texSize);
      if (previousDepthDelta <= 0.0 && depthDelta >= 0.0 &&
          depthDelta <= hitThickness && screenDistance > 1.5) {
        let sampleNormal = normalize(
          textureSampleLevel(normalTexture, normalTextureSampler, sampleCoord, 0).rgb * 2.0 - 1.0
        );
        let emitterFacing = max(dot(sampleNormal, -rayDirection), 0.0);
        let receiverFacing = max(dot(centerNormal, rayDirection), 0.0);
        let distanceFalloff = 1.0 - smoothstep(
          ssgiTrace.radius * 0.28,
          ssgiTrace.radius,
          travel
        );
        let edgeDistance = min(
          min(sampleCoord.x, sampleCoord.y),
          min(1.0 - sampleCoord.x, 1.0 - sampleCoord.y)
        );
        let edgeWeight = smoothstep(0.0, 0.075, edgeDistance);
        let sampleWeight = receiverFacing * (0.2 + emitterFacing * 0.8) *
          distanceFalloff * edgeWeight;
        let sourceRadiance = textureSampleLevel(
          sourceTexture,
          sourceTextureSampler,
          sampleCoord,
          0
        ).rgb;
        indirectRadiance += sourceRadiance * sampleWeight;
        accumulatedWeight += sampleWeight;
        hitCount += select(0.0, 1.0, sampleWeight > 0.0001);
        break;
      }
      previousDepthDelta = depthDelta;
    }
  }

  let diffuseResponse = mix(0.18, 1.0, roughness);
  let radiance = indirectRadiance /
    max(f32(activeRayCount), 1.0) * ssgiTrace.intensity * diffuseResponse;
  let confidence = clamp(hitCount / max(f32(activeRayCount), 1.0), 0.0, 1.0);
  return vec4f(radiance, confidence);
}
