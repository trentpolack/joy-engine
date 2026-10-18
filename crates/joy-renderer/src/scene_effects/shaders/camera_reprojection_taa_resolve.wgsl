// Ported from luma.gl 9.4.2 (@luma.gl/effects src/passes/screen-space/camera-reprojection-temporal-antialiasing.ts).
// Copyright (c) vis.gl contributors. MIT License; see LICENSE-luma.gl beside this folder.

const CAMERA_REPROJECTION_TAA_EPSILON: f32 = 0.00001;

struct CameraReprojectionTaaResolveUniforms {
  inverseViewProjectionMatrix: mat4x4f,
  previousViewProjectionMatrix: mat4x4f,
  historyWeight: f32,
  depthThreshold: f32,
  currentJitter: vec2f,
  previousJitter: vec2f,
};

@group(0) @binding(auto) var<uniform> cameraReprojectionTaaResolve:
  CameraReprojectionTaaResolveUniforms;
@group(0) @binding(auto) var historyTexture: texture_2d<f32>;
@group(0) @binding(auto) var depthTexture: texture_depth_2d;
@group(0) @binding(auto) var depthTextureSampler: sampler;
@group(0) @binding(auto) var previousDepthTexture: texture_2d<f32>;

fn cameraReprojectionTaaResolve_previousFrameCoordinate(
  texCoord: vec2f,
  depth: f32
) -> vec4f {
  let unjitteredCoordinate = texCoord - cameraReprojectionTaaResolve.currentJitter;
  if (any(unjitteredCoordinate < vec2f(0.0)) ||
      any(unjitteredCoordinate > vec2f(1.0))) {
    return vec4f(0.0);
  }

  let currentClip = vec4f(
    unjitteredCoordinate.x * 2.0 - 1.0,
    1.0 - unjitteredCoordinate.y * 2.0,
    depth,
    1.0
  );
  let worldPositionHomogeneous =
    cameraReprojectionTaaResolve.inverseViewProjectionMatrix * currentClip;
  if (abs(worldPositionHomogeneous.w) <= CAMERA_REPROJECTION_TAA_EPSILON) {
    return vec4f(0.0);
  }

  let worldPosition = worldPositionHomogeneous.xyz / worldPositionHomogeneous.w;
  let previousClip =
    cameraReprojectionTaaResolve.previousViewProjectionMatrix * vec4f(worldPosition, 1.0);
  if (previousClip.w <= CAMERA_REPROJECTION_TAA_EPSILON) {
    return vec4f(0.0);
  }

  let previousNormalizedDeviceCoordinate = previousClip.xyz / previousClip.w;
  let previousCoordinate =
    previousNormalizedDeviceCoordinate.xy * vec2f(0.5, -0.5) +
    vec2f(0.5) +
    cameraReprojectionTaaResolve.previousJitter;
  let expectedPreviousDepth = previousNormalizedDeviceCoordinate.z;
  let validCoordinate = all(previousCoordinate >= vec2f(0.0)) &&
    all(previousCoordinate <= vec2f(1.0));
  let validDepthRange = expectedPreviousDepth >= 0.0 && expectedPreviousDepth <= 1.0;
  return vec4f(
    previousCoordinate,
    expectedPreviousDepth,
    select(0.0, 1.0, validCoordinate && validDepthRange)
  );
}

fn cameraReprojectionTaaResolve_sampleColor(
  sourceTexture: texture_2d<f32>,
  sourceTextureSampler: sampler,
  texSize: vec2f,
  texCoord: vec2f
) -> vec4f {
  let currentColor = textureSampleLevel(sourceTexture, sourceTextureSampler, texCoord, 0);
  let currentDepth = textureSampleLevel(depthTexture, depthTextureSampler, texCoord, 0);
  if (currentDepth >= 0.99999) {
    return currentColor;
  }

  let previousFrame = cameraReprojectionTaaResolve_previousFrameCoordinate(
    texCoord,
    currentDepth
  );
  if (previousFrame.w < 0.5) {
    return currentColor;
  }

  let sourceTexel = 1.0 / vec2f(textureDimensions(sourceTexture));
  var minimumColor = currentColor.rgb;
  var maximumColor = currentColor.rgb;
  for (var sampleY: i32 = -1; sampleY <= 1; sampleY++) {
    for (var sampleX: i32 = -1; sampleX <= 1; sampleX++) {
      let sampleCoordinate = clamp(
        texCoord + vec2f(f32(sampleX), f32(sampleY)) * sourceTexel,
        vec2f(0.0),
        vec2f(1.0)
      );
      let sampleColor = textureSampleLevel(
        sourceTexture,
        sourceTextureSampler,
        sampleCoordinate,
        0
      ).rgb;
      minimumColor = min(minimumColor, sampleColor);
      maximumColor = max(maximumColor, sampleColor);
    }
  }

  let historyDimensions = textureDimensions(historyTexture);
  let historyPosition = previousFrame.xy * vec2f(historyDimensions) - vec2f(0.5);
  let baseHistoryCoordinate = vec2i(floor(historyPosition));
  let historyFraction = fract(historyPosition);
  var accumulatedHistoryColor = vec3f(0.0);
  var accumulatedHistoryWeight = 0.0;
  for (var tapY: i32 = 0; tapY <= 1; tapY++) {
    for (var tapX: i32 = 0; tapX <= 1; tapX++) {
      let historyCoordinate = clamp(
        baseHistoryCoordinate + vec2i(tapX, tapY),
        vec2i(0),
        vec2i(historyDimensions) - vec2i(1)
      );
      let tapDepth = textureLoad(previousDepthTexture, historyCoordinate, 0).r;
      let validTapDepth =
        abs(tapDepth - previousFrame.z) <= cameraReprojectionTaaResolve.depthThreshold;
      let horizontalWeight = select(1.0 - historyFraction.x, historyFraction.x, tapX == 1);
      let verticalWeight = select(1.0 - historyFraction.y, historyFraction.y, tapY == 1);
      let tapWeight = select(0.0, horizontalWeight * verticalWeight, validTapDepth);
      accumulatedHistoryColor += textureLoad(historyTexture, historyCoordinate, 0).rgb *
        tapWeight;
      accumulatedHistoryWeight += tapWeight;
    }
  }

  if (accumulatedHistoryWeight <= CAMERA_REPROJECTION_TAA_EPSILON) {
    return currentColor;
  }

  let historyColor = accumulatedHistoryColor / accumulatedHistoryWeight;
  let clampedHistoryColor = clamp(historyColor, minimumColor, maximumColor);
  let resolvedColor = mix(
    currentColor.rgb,
    clampedHistoryColor,
    cameraReprojectionTaaResolve.historyWeight
  );
  return vec4f(resolvedColor, currentColor.a);
}
