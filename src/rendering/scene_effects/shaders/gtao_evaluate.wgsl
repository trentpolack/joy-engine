// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/gtao.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

const GTAO_PI: f32 = 3.141592653589793;
const GTAO_SLICE_COUNT: i32 = 4;
const GTAO_STEPS_PER_SIDE: i32 = 6;

struct GTAOEvaluateUniforms {
  projectionMatrix: mat4x4f,
  inverseProjectionMatrix: mat4x4f,
  radius: f32,
  bias: f32,
  intensity: f32,
  frameIndex: f32,
};

@group(0) @binding(auto) var<uniform> gtaoEvaluate: GTAOEvaluateUniforms;
@group(0) @binding(auto) var depthTexture: texture_depth_2d;
@group(0) @binding(auto) var depthTextureSampler: sampler;
@group(0) @binding(auto) var normalTexture: texture_2d<f32>;
@group(0) @binding(auto) var normalTextureSampler: sampler;

fn gtaoEvaluate_reconstructViewPosition(uv: vec2f, depth: f32) -> vec3f {
  let clip = vec4f(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
  let viewPosition = gtaoEvaluate.inverseProjectionMatrix * clip;
  return viewPosition.xyz / max(viewPosition.w, 0.00001);
}

fn gtaoEvaluate_hash(value: vec2f) -> f32 {
  return fract(sin(dot(value, vec2f(12.9898, 78.233))) * 43758.5453);
}

fn gtaoEvaluate_sampleHorizon(
  centerPosition: vec3f,
  viewDirection: vec3f,
  sliceDirection: vec3f,
  sceneCoord: vec2f,
  direction: vec2f,
  side: f32,
  radiusPixels: f32,
  depthDimensions: vec2f,
  radialJitter: f32
) -> f32 {
  let hemisphereEdge = side * GTAO_PI * 0.5;
  var horizon = hemisphereEdge;
  for (var stepIndex: i32 = 1; stepIndex <= GTAO_STEPS_PER_SIDE; stepIndex++) {
    let fraction = f32(stepIndex) / f32(GTAO_STEPS_PER_SIDE);
    let jitteredFraction =
      (f32(stepIndex) - 1.0 + radialJitter) / f32(GTAO_STEPS_PER_SIDE);
    let sampleOffset = direction * side * radiusPixels * jitteredFraction / depthDimensions;
    let sampleCoord = sceneCoord + sampleOffset;
    if (any(sampleCoord < vec2f(0.0)) || any(sampleCoord > vec2f(1.0))) {
      continue;
    }
    let sampleDepth = textureSampleLevel(depthTexture, depthTextureSampler, sampleCoord, 0);
    if (sampleDepth >= 0.99999) {
      continue;
    }
    let samplePosition = gtaoEvaluate_reconstructViewPosition(sampleCoord, sampleDepth);
    let delta = samplePosition - centerPosition;
    let distanceToSample = length(delta);
    if (distanceToSample <= 0.0001 || distanceToSample >= gtaoEvaluate.radius) {
      continue;
    }
    let sampleDirection = delta / distanceToSample;
    let sliceProjection = dot(sampleDirection, sliceDirection);
    if (sliceProjection * side <= 0.0) {
      continue;
    }
    let sampleAngle = atan2(sliceProjection, dot(sampleDirection, viewDirection));
    let biasedAngle = sampleAngle + side * gtaoEvaluate.bias;
    let boundedAngle = select(
      clamp(biasedAngle, -GTAO_PI * 0.5, 0.0),
      clamp(biasedAngle, 0.0, GTAO_PI * 0.5),
      side > 0.0
    );
    let distanceFalloff = 1.0 - smoothstep(gtaoEvaluate.radius * 0.35, gtaoEvaluate.radius, distanceToSample);
    let stepFalloff = 1.0 - fraction * 0.15;
    let weightedAngle = mix(hemisphereEdge, boundedAngle, distanceFalloff * stepFalloff);
    horizon = select(max(horizon, weightedAngle), min(horizon, weightedAngle), side > 0.0);
  }
  return horizon;
}

fn gtaoEvaluate_integrateSlice(
  negativeHorizon: f32,
  positiveHorizon: f32,
  normalAngle: f32
) -> f32 {
  let lowerHemisphere = normalAngle - GTAO_PI * 0.5;
  let upperHemisphere = normalAngle + GTAO_PI * 0.5;
  let visibleLower = clamp(negativeHorizon, lowerHemisphere, upperHemisphere);
  let visibleUpper = clamp(positiveHorizon, lowerHemisphere, upperHemisphere);
  let unoccludedLower = clamp(-GTAO_PI * 0.5, lowerHemisphere, upperHemisphere);
  let unoccludedUpper = clamp(GTAO_PI * 0.5, lowerHemisphere, upperHemisphere);
  let visibilityIntegral = max(
    sin(visibleUpper - normalAngle) - sin(visibleLower - normalAngle),
    0.0
  );
  let hemisphereIntegral = max(
    sin(unoccludedUpper - normalAngle) - sin(unoccludedLower - normalAngle),
    0.0001
  );
  return clamp(visibilityIntegral / hemisphereIntegral, 0.0, 1.0);
}

fn gtaoEvaluate_sampleColor(
  sourceTexture: texture_2d<f32>,
  sourceTextureSampler: sampler,
  texSize: vec2f,
  texCoord: vec2f
) -> vec4f {
  let sceneCoord = texCoord;
  let depth = textureSampleLevel(depthTexture, depthTextureSampler, sceneCoord, 0);
  if (depth >= 0.99999) {
    return vec4f(1.0);
  }

  let centerPosition = gtaoEvaluate_reconstructViewPosition(sceneCoord, depth);
  let normalSample = textureSampleLevel(normalTexture, normalTextureSampler, sceneCoord, 0);
  let normal = normalize(normalSample.xyz * 2.0 - 1.0);
  let depthDimensions = vec2f(textureDimensions(depthTexture));
  let focalLength = abs(gtaoEvaluate.projectionMatrix[1][1]);
  let projectedRadius = gtaoEvaluate.radius * focalLength /
    max(-centerPosition.z, 0.0001) * depthDimensions.y * 0.5;
  let radiusPixels = clamp(projectedRadius, 2.0, 72.0);
  let pixelCoordinate = sceneCoord * depthDimensions;
  let frameOffset = vec2f(gtaoEvaluate.frameIndex * 0.754877666, gtaoEvaluate.frameIndex * 0.569840291);
  let rotation = gtaoEvaluate_hash(floor(pixelCoordinate) + frameOffset) * GTAO_PI;
  let radialJitter = 0.25 + gtaoEvaluate_hash(floor(pixelCoordinate) + frameOffset.yx + 17.0) * 0.75;
  let viewDirection = normalize(-centerPosition);

  var accumulatedVisibility = 0.0;
  var accumulatedWeight = 0.0;
  for (var sliceIndex: i32 = 0; sliceIndex < GTAO_SLICE_COUNT; sliceIndex++) {
    let angle = rotation + f32(sliceIndex) * GTAO_PI / f32(GTAO_SLICE_COUNT);
    let direction = vec2f(cos(angle), sin(angle));
    let sliceSampleCoord = clamp(sceneCoord + direction / depthDimensions, vec2f(0.0), vec2f(1.0));
    let slicePosition = gtaoEvaluate_reconstructViewPosition(sliceSampleCoord, depth);
    let sliceDelta = slicePosition - centerPosition;
    if (length(sliceDelta) <= 0.00001) {
      continue;
    }
    let slicePlaneNormal = normalize(cross(sliceDelta, viewDirection));
    let sliceDirection = normalize(cross(viewDirection, slicePlaneNormal));
    let projectedNormal = normal - slicePlaneNormal * dot(normal, slicePlaneNormal);
    let projectedNormalLength = length(projectedNormal);
    if (projectedNormalLength <= 0.00001) {
      continue;
    }
    let normalizedProjectedNormal = projectedNormal / projectedNormalLength;
    let normalAngle = atan2(
      dot(normalizedProjectedNormal, sliceDirection),
      dot(normalizedProjectedNormal, viewDirection)
    );
    let positiveHorizon = gtaoEvaluate_sampleHorizon(
      centerPosition,
      viewDirection,
      sliceDirection,
      sceneCoord,
      direction,
      1.0,
      radiusPixels,
      depthDimensions,
      radialJitter
    );
    let negativeHorizon = gtaoEvaluate_sampleHorizon(
      centerPosition,
      viewDirection,
      sliceDirection,
      sceneCoord,
      direction,
      -1.0,
      radiusPixels,
      depthDimensions,
      radialJitter
    );
    let sliceVisibility = gtaoEvaluate_integrateSlice(
      negativeHorizon,
      positiveHorizon,
      normalAngle
    );
    accumulatedVisibility += sliceVisibility * projectedNormalLength;
    accumulatedWeight += projectedNormalLength;
  }

  let integratedVisibility = accumulatedVisibility / max(accumulatedWeight, 0.0001);
  let visibility = clamp(
    1.0 - (1.0 - integratedVisibility) * gtaoEvaluate.intensity,
    0.0,
    1.0
  );
  return vec4f(vec3f(visibility), 1.0);
}
