// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { SCENE_EFFECT, SCENE_EFFECT_PROPERTY, SCENE_NORMAL_SOURCE, SCENE_REPROJECTION } from './constants.ts';
import { GPU_TEXTURE_FORMAT } from '../gpu/constants.ts';

/**
 * Perspective-only ambient occlusion. Radius is measured in drawing-buffer pixels.
 */
export type SceneSsaoOptions = {
  radius?: number;
  bias?: number;
  intensity?: number;
  resolutionScale?: number;
  normalSource?: typeof SCENE_NORMAL_SOURCE.RECONSTRUCT_FROM_DEPTH | typeof SCENE_NORMAL_SOURCE.NORMAL_TEXTURE;
};
/**
 * Bilateral scene-color blur. Radius and spatial sigma are measured in pixels; depth sigma uses hardware depth.
 */
export type SceneDepthAwareBlurOptions = {
  radius?: number;
  depthSigma?: number;
  spatialSigma?: number;
};
/**
 * Temporally stabilized horizon occlusion. Radius is measured in scene units.
 */
export type SceneGtaoOptions = {
  radius?: number;
  bias?: number;
  intensity?: number;
  resolutionScale?: number;
  historyWeight?: number;
  depthThreshold?: number;
  strength?: number;
};
/**
 * Colored diffuse screen-space illumination. Radius and thickness are measured in scene units.
 */
export type SceneSsgiOptions = {
  radius?: number;
  thickness?: number;
  intensity?: number;
  rayCount?: number;
  stepCount?: number;
  resolutionScale?: number;
  historyWeight?: number;
  depthThreshold?: number;
  strength?: number;
};
/**
 * Depth/normal edges. Thickness is measured in drawing-buffer pixels; color is linear RGBA.
 */
export type SceneOutlineOptions = {
  color?: [number, number, number, number];
  thickness?: number;
  depthThreshold?: number;
  normalThreshold?: number;
  normalSource?: typeof SCENE_NORMAL_SOURCE.RECONSTRUCT_FROM_DEPTH | typeof SCENE_NORMAL_SOURCE.NORMAL_TEXTURE;
};
/**
 * Blur using unjittered current-minus-previous UV motion. Strength scales that motion.
 */
export type SceneMotionBlurOptions = {
  strength?: number;
  sampleCount?: number;
};
/**
 * Velocity reprojection supports moving geometry; camera reprojection assumes a static world.
 */
export type SceneTaaOptions = {
  historyWeight?: number;
  depthThreshold?: number;
  reprojection?: typeof SCENE_REPROJECTION.VELOCITY | typeof SCENE_REPROJECTION.CAMERA;
};
/**
 * Roughness-aware reflections. Max distance and thickness are measured in scene units.
 */
export type SceneSsrOptions = {
  intensity?: number;
  maxDistance?: number;
  thickness?: number;
  sampleCount?: number;
  maxRoughness?: number;
  resolutionScale?: number;
  historyWeight?: number;
  depthThreshold?: number;
  strength?: number;
};
/**
 * Compact screen-space height approximation. History is disabled because upstream cannot reproject it.
 */
export type SceneHeightFogOptions = {
  fogColor?: [number, number, number, number];
  density?: number;
  heightFalloff?: number;
  scattering?: number;
};
/**
 * Bounded participating-media integration. Distances and fog height use scene units.
 */
export type SceneClusteredLightingOptions = {
  resolutionScale?: number;
  fogColor?: [number, number, number];
  density?: number;
  heightFalloff?: number;
  fogHeight?: number;
  anisotropy?: number;
  directionalIntensity?: number;
  pointLightIntensity?: number;
  maxDistance?: number;
  sampleCount?: number;
  shadowStrength?: number;
  historyWeight?: number;
  depthThreshold?: number;
  strength?: number;
};
/**
 * GPU-resident luminance metering. Adaptation speeds are rates per second.
 */
export type SceneAdaptiveExposureOptions = {
  meteringScale?: number;
  initialExposure?: number;
  keyValue?: number;
  minimumExposure?: number;
  maximumExposure?: number;
  brightenSpeed?: number;
  darkenSpeed?: number;
};
/**
 * Each effect accepts true for defaults, false to disable, or a deliberately narrow tuning record.
 */
export type SceneEffectsOptions = {
  colorFormat?: typeof GPU_TEXTURE_FORMAT.RGBA16FLOAT | typeof GPU_TEXTURE_FORMAT.RGBA8UNORM;
  depthAwareBlur?: boolean | SceneDepthAwareBlurOptions;
  ssao?: boolean | SceneSsaoOptions;
  gtao?: boolean | SceneGtaoOptions;
  ssgi?: boolean | SceneSsgiOptions;
  outlines?: boolean | SceneOutlineOptions;
  taa?: boolean | SceneTaaOptions;
  motionBlur?: boolean | SceneMotionBlurOptions;
  ssr?: boolean | SceneSsrOptions;
  heightFog?: boolean | SceneHeightFogOptions;
  clusteredLighting?: boolean | SceneClusteredLightingOptions;
  adaptiveExposure?: boolean | SceneAdaptiveExposureOptions;
};
export type NormalizedSsaoOptions = Readonly<Required<SceneSsaoOptions>>;
export type NormalizedMotionBlurOptions = Readonly<Required<SceneMotionBlurOptions>>;
export type NormalizedTaaOptions = Readonly<Required<SceneTaaOptions>>;
export type NormalizedSceneEffectsOptions = Readonly<{
  colorFormat: typeof GPU_TEXTURE_FORMAT.RGBA16FLOAT | typeof GPU_TEXTURE_FORMAT.RGBA8UNORM | null;
  depthAwareBlur: Readonly<Required<SceneDepthAwareBlurOptions>> | null;
  ssao: NormalizedSsaoOptions | null;
  gtao: Readonly<Required<SceneGtaoOptions>> | null;
  ssgi: Readonly<Required<SceneSsgiOptions>> | null;
  outlines: Readonly<Required<SceneOutlineOptions>> | null;
  taa: NormalizedTaaOptions | null;
  motionBlur: NormalizedMotionBlurOptions | null;
  ssr: Readonly<Required<SceneSsrOptions>> | null;
  heightFog: Readonly<Required<SceneHeightFogOptions>> | null;
  clusteredLighting: Readonly<Required<SceneClusteredLightingOptions>> | null;
  adaptiveExposure: Readonly<Required<SceneAdaptiveExposureOptions>> | null;
}>;

export const SCENE_EFFECT_NAMES = [
  SCENE_EFFECT.DEPTH_AWARE_BLUR, SCENE_EFFECT.SSAO, SCENE_EFFECT.GTAO, SCENE_EFFECT.SSGI, SCENE_EFFECT.OUTLINES, SCENE_EFFECT.TAA, SCENE_EFFECT.MOTION_BLUR, SCENE_EFFECT.SSR,
  SCENE_EFFECT.HEIGHT_FOG, SCENE_EFFECT.CLUSTERED_LIGHTING, SCENE_EFFECT.ADAPTIVE_EXPOSURE
] as const;

const DEFAULTS = {
  depthAwareBlur: { radius: 4, depthSigma: 0.01, spatialSigma: 3 },
  ssao: { radius: 7, bias: 0.03, intensity: 1.35, resolutionScale: 1, normalSource: SCENE_NORMAL_SOURCE.RECONSTRUCT_FROM_DEPTH },
  gtao: {
    radius: 2.2,
    bias: 0.04,
    intensity: 3.2,
    resolutionScale: 1,
    historyWeight: 0.85,
    depthThreshold: 0.02,
    strength: 1
  },
  ssgi: {
    radius: 4.5,
    thickness: 0.32,
    intensity: 2.2,
    rayCount: 7,
    stepCount: 8,
    resolutionScale: 0.5,
    historyWeight: 0.85,
    depthThreshold: 0.02,
    strength: 1
  },
  outlines: {
    color: [0.02, 0.025, 0.035, 1],
    thickness: 1,
    depthThreshold: 0.01,
    normalThreshold: 0.2,
    normalSource: SCENE_NORMAL_SOURCE.RECONSTRUCT_FROM_DEPTH
  },
  taa: { historyWeight: 0.9, depthThreshold: 0.01, reprojection: SCENE_REPROJECTION.VELOCITY },
  motionBlur: { strength: 1, sampleCount: 10 },
  ssr: {
    intensity: 1.35,
    maxDistance: 60,
    thickness: 0.45,
    sampleCount: 48,
    maxRoughness: 0.88,
    resolutionScale: 0.5,
    historyWeight: 0.85,
    depthThreshold: 0.02,
    strength: 1
  },
  heightFog: { fogColor: [0.18, 0.34, 0.48, 0.6], density: 0.22, heightFalloff: 3, scattering: 0.35 },
  clusteredLighting: {
    resolutionScale: 0.5,
    fogColor: [0.18, 0.3, 0.48],
    density: 0.055,
    heightFalloff: 0.23,
    fogHeight: 0.2,
    anisotropy: 0.45,
    directionalIntensity: 2.2,
    pointLightIntensity: 1.8,
    maxDistance: 28,
    sampleCount: 10,
    shadowStrength: 0.74,
    historyWeight: 0.85,
    depthThreshold: 0.02,
    strength: 1
  },
  adaptiveExposure: {
    meteringScale: 0.25,
    initialExposure: 1,
    keyValue: 0.18,
    minimumExposure: 0.125,
    maximumExposure: 8,
    brightenSpeed: 1.5,
    darkenSpeed: 3
  }
} satisfies { [K in typeof SCENE_EFFECT_NAMES[number]]: Required<Exclude<SceneEffectsOptions[K], boolean | undefined>> };

/** Validate and snapshot all tuning before allocating GPU resources. */
export function normalizeSceneEffectsOptions(options: SceneEffectsOptions = {}): NormalizedSceneEffectsOptions {
  assertRecord(options, 'Scene effects options');
  assertKnownKeys(options, ['colorFormat', ...SCENE_EFFECT_NAMES], 'Scene effects options');
  if(options.colorFormat !== undefined && options.colorFormat !== GPU_TEXTURE_FORMAT.RGBA16FLOAT && options.colorFormat !== GPU_TEXTURE_FORMAT.RGBA8UNORM) {
    throw new TypeError('colorFormat must be rgba16float or rgba8unorm.');
  }
  const normalized: Record<string, unknown> = { colorFormat: options.colorFormat ?? null };
  for(const name of SCENE_EFFECT_NAMES) {
    const value = options[name];
    if(value === false || value === undefined) {
      normalized[name] = null;
      continue;
    }
    const defaults = DEFAULTS[name];
    if(value !== true) {
      assertRecord(value, name);
      assertKnownKeys(value, Object.keys(defaults), name);
    }
    const settings = { ...defaults, ...(value === true ? {} : value) };
    validateSettings(name, settings);
    // Copy vectors so author mutation cannot change an active render graph.
    for(const [key, setting] of Object.entries(settings)) {
      if(Array.isArray(setting)) {
        Object.assign(settings, { [key]: Object.freeze([...setting]) });
      }
    }
    normalized[name] = Object.freeze(settings);
  }
  if(normalized.ssao && normalized.gtao) {
    throw new Error('ssao and gtao are mutually exclusive ambient-occlusion implementations.');
  }
  const exposure = normalized.adaptiveExposure as Required<SceneAdaptiveExposureOptions> | null;
  if(exposure && exposure.minimumExposure > exposure.maximumExposure) {
    throw new RangeError('adaptiveExposure.minimumExposure must not exceed maximumExposure.');
  }
  // Every effect shape above was checked against its own defaults and validated.
  return Object.freeze(normalized) as NormalizedSceneEffectsOptions;
}

/** Attachment requirements for allocation by a scene renderer; independent of backend availability. */
export function getSceneEffectsRequirements(options: NormalizedSceneEffectsOptions) {
  const normalTexture = Boolean(options.gtao || options.ssgi || options.ssr ||
    options.ssao?.normalSource === SCENE_NORMAL_SOURCE.NORMAL_TEXTURE || options.outlines?.normalSource === SCENE_NORMAL_SOURCE.NORMAL_TEXTURE);
  return Object.freeze({
    depthTexture: SCENE_EFFECT_NAMES.some(name => name !== SCENE_EFFECT.ADAPTIVE_EXPOSURE && Boolean(options[name])),
    normalTexture,
    velocityTexture: Boolean(options.gtao || options.ssgi || options.ssr || options.clusteredLighting),
    taaVelocityTexture: Boolean(options.motionBlur || options.taa?.reprojection === SCENE_REPROJECTION.VELOCITY),
    preciseDepthHistory: Boolean(options.gtao || options.ssgi || options.ssr || options.taa),
    cameraMatrices: Boolean(options.gtao || options.ssgi || options.ssr || options.clusteredLighting || options.taa?.reprojection === SCENE_REPROJECTION.CAMERA),
    clusteredLighting: Boolean(options.clusteredLighting)
  });
}

function validateSettings(name: string, settings: object) {
  for(const [key, value] of Object.entries(settings)) {
    const label = `${name}.${key}`;
    if(key === SCENE_EFFECT_PROPERTY.NORMAL_SOURCE) {
      if(value !== SCENE_NORMAL_SOURCE.RECONSTRUCT_FROM_DEPTH && value !== SCENE_NORMAL_SOURCE.NORMAL_TEXTURE) {
        throw new TypeError(`${label} must be reconstruct-from-depth or normal-texture.`);
      }
      continue;
    }
    if(key === SCENE_EFFECT_PROPERTY.REPROJECTION) {
      if(value !== SCENE_REPROJECTION.VELOCITY && value !== SCENE_REPROJECTION.CAMERA) {
        throw new TypeError(`${label} must be velocity or camera.`);
      }
      continue;
    }
    if(key === SCENE_EFFECT_PROPERTY.COLOR || key === SCENE_EFFECT_PROPERTY.FOG_COLOR) {
      const length = name === SCENE_EFFECT.CLUSTERED_LIGHTING ? 3 : 4;
      assertVector(value, length, label);
      for(const channel of value) {
        assertRange(channel, 0, 1, label);
      }
      continue;
    }
    let minimum = 0;
    let maximum = Infinity;
    if([
      SCENE_EFFECT_PROPERTY.DEPTH_SIGMA,
      SCENE_EFFECT_PROPERTY.SPATIAL_SIGMA,
      SCENE_EFFECT_PROPERTY.DEPTH_THRESHOLD,
      SCENE_EFFECT_PROPERTY.THICKNESS,
      SCENE_EFFECT_PROPERTY.INITIAL_EXPOSURE,
      SCENE_EFFECT_PROPERTY.KEY_VALUE,
      SCENE_EFFECT_PROPERTY.MINIMUM_EXPOSURE,
      SCENE_EFFECT_PROPERTY.MAXIMUM_EXPOSURE
    ].some(property => property === key)) {
      minimum = Number.MIN_VALUE;
    }
    if(key === SCENE_EFFECT_PROPERTY.RESOLUTION_SCALE || key === SCENE_EFFECT_PROPERTY.METERING_SCALE) {
      minimum = 0.25;
      maximum = 1;
    }
    if(key === SCENE_EFFECT_PROPERTY.HISTORY_WEIGHT) {
      maximum = 0.98;
    }
    if(key === SCENE_EFFECT_PROPERTY.MAX_ROUGHNESS || key === SCENE_EFFECT_PROPERTY.SHADOW_STRENGTH) {
      maximum = 1;
    }
    if(key === SCENE_EFFECT_PROPERTY.ANISOTROPY) {
      minimum = -0.8;
      maximum = 0.85;
    }
    if(key === SCENE_EFFECT_PROPERTY.FOG_HEIGHT) {
      minimum = -Infinity;
    }
    if(key === SCENE_EFFECT_PROPERTY.RADIUS) {
      minimum = name === SCENE_EFFECT.SSAO || name === SCENE_EFFECT.DEPTH_AWARE_BLUR ? 1 : 0.1;
    }
    if(key === SCENE_EFFECT_PROPERTY.RADIUS && name === SCENE_EFFECT.DEPTH_AWARE_BLUR) {
      maximum = 8;
    }
    if(key === SCENE_EFFECT_PROPERTY.MAX_DISTANCE) {
      minimum = 1;
      maximum = 1000;
    }
    if(key === SCENE_EFFECT_PROPERTY.SAMPLE_COUNT) {
      minimum = name === SCENE_EFFECT.MOTION_BLUR ? 2 : name === SCENE_EFFECT.SSR ? 8 : 3;
      maximum = name === SCENE_EFFECT.MOTION_BLUR ? 16 : name === SCENE_EFFECT.SSR ? 96 : 20;
    }
    if(key === SCENE_EFFECT_PROPERTY.RAY_COUNT) {
      minimum = 1;
      maximum = 12;
    }
    if(key === SCENE_EFFECT_PROPERTY.STEP_COUNT) {
      minimum = 2;
      maximum = 12;
    }
    assertRange(value, minimum, maximum, label);
    if([SCENE_EFFECT_PROPERTY.SAMPLE_COUNT, SCENE_EFFECT_PROPERTY.RAY_COUNT, SCENE_EFFECT_PROPERTY.STEP_COUNT].some(property => property === key) || (key === SCENE_EFFECT_PROPERTY.RADIUS && name === SCENE_EFFECT.DEPTH_AWARE_BLUR)) {
      if(!Number.isInteger(value)) {
        throw new RangeError(`${label} must be an integer.`);
      }
    }
  }
}

export function assertRecord(value: unknown, name: string): asserts value is Record<string, unknown> {
  if(!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new TypeError(`${name} must be an object.`);
  }
}
export function assertKnownKeys(record: object, keys: readonly string[], name: string) {
  for(const key of Object.keys(record)) {
    if(!keys.includes(key)) {
      throw new TypeError(`${name} contains unknown property ${key}.`);
    }
  }
}
export function assertRange(value: unknown, minimum: number, maximum: number, name: string): asserts value is number {
  if(typeof value !== 'number' || !Number.isFinite(value) || !Number.isFinite(Math.fround(value)) || value < minimum || value > maximum) {
    throw new RangeError(`${name} must be a finite number from ${minimum} through ${maximum}.`);
  }
}
export function assertVector(value: unknown, length: number, name: string): asserts value is number[] {
  if(!Array.isArray(value) || value.length !== length) {
    throw new TypeError(`${name} must be a ${length}-number array.`);
  }
  for(const channel of value) {
    assertRange(channel, -Infinity, Infinity, name);
  }
}
