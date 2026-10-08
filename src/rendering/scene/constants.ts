// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Scene effect identifiers used by configuration, renderer and controls.
 */
export const SCENE_EFFECT = Object.freeze({
  DEPTH_AWARE_BLUR: 'depthAwareBlur',
  SSAO: 'ssao',
  GTAO: 'gtao',
  SSGI: 'ssgi',
  OUTLINES: 'outlines',
  TAA: 'taa',
  MOTION_BLUR: 'motionBlur',
  SSR: 'ssr',
  HEIGHT_FOG: 'heightFog',
  CLUSTERED_LIGHTING: 'clusteredLighting',
  ADAPTIVE_EXPOSURE: 'adaptiveExposure'
} as const);

/**
 * Normal-buffer sources accepted by screen-space effects.
 */
export const SCENE_NORMAL_SOURCE = Object.freeze({
  RECONSTRUCT_FROM_DEPTH: 'reconstruct-from-depth',
  NORMAL_TEXTURE: 'normal-texture'
} as const);

/**
 * Temporal reprojection methods.
 */
export const SCENE_REPROJECTION = Object.freeze({
  CAMERA: 'camera',
  VELOCITY: 'velocity'
} as const);

/**
 * Validated scene effect option names.
 */
export const SCENE_EFFECT_PROPERTY = Object.freeze({
  NORMAL_SOURCE: 'normalSource',
  REPROJECTION: 'reprojection',
  COLOR: 'color',
  FOG_COLOR: 'fogColor',
  DEPTH_SIGMA: 'depthSigma',
  SPATIAL_SIGMA: 'spatialSigma',
  DEPTH_THRESHOLD: 'depthThreshold',
  THICKNESS: 'thickness',
  INITIAL_EXPOSURE: 'initialExposure',
  KEY_VALUE: 'keyValue',
  MINIMUM_EXPOSURE: 'minimumExposure',
  MAXIMUM_EXPOSURE: 'maximumExposure',
  RESOLUTION_SCALE: 'resolutionScale',
  METERING_SCALE: 'meteringScale',
  HISTORY_WEIGHT: 'historyWeight',
  MAX_ROUGHNESS: 'maxRoughness',
  SHADOW_STRENGTH: 'shadowStrength',
  ANISOTROPY: 'anisotropy',
  FOG_HEIGHT: 'fogHeight',
  RADIUS: 'radius',
  MAX_DISTANCE: 'maxDistance',
  SAMPLE_COUNT: 'sampleCount',
  RAY_COUNT: 'rayCount',
  STEP_COUNT: 'stepCount'
} as const);
