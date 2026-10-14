// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {
  Texture,
  Device
} from '@luma.gl/core';
import type { ShaderPassRendererRenderOptions } from '@luma.gl/engine';
import type { SceneVolumeLightBindings } from '../lighting/scene-volume-lights.ts';
import type {
  SceneEffectsOptions,
  NormalizedSceneEffectsOptions
} from './scene-effect-config.ts';

import {
  SCENE_EFFECT,
  SCENE_REPROJECTION
} from './constants.ts';
import {
  GPU_BACKEND,
  GPU_TEXTURE_FORMAT
} from '../gpu/constants.ts';
import { ShaderPassRenderer } from '../gpu/shader-pass-renderer.ts';
import {
  normalizeSceneEffectsOptions,
  getSceneEffectsRequirements,
  SCENE_EFFECT_NAMES,
  assertRecord,
  assertKnownKeys,
  assertRange,
  assertVector
} from './scene-effect-config.ts';
import {
  createSceneEffectPipelines,
  validateSceneEffectFormats
} from './scene-effect-pipelines.ts';

export type * from './scene-effect-config.ts';
export {
  normalizeSceneEffectsOptions,
  getSceneEffectsRequirements
} from './scene-effect-config.ts';

/**
 * All textures, buffers, and column-major camera matrices are borrowed for this frame.
 * Depth and reconstructed matrices use WebGPU [0,1] depth and top-left UVs.
 * normalTexture packs encoded view normals in RGB and perceptual roughness in A.
 * velocityTexture is current minus previous jittered UV; taaVelocityTexture excludes jitter.
 * Projection matrices reconstruct the rasterized depth; camera reprojection matrices are unjittered.
 */
export type SceneEffectsFrame = {
  depthTexture?: Texture;
  normalTexture?: Texture;
  velocityTexture?: Texture;
  taaVelocityTexture?: Texture;
  projectionMatrix?: ArrayLike<number>;
  inverseProjectionMatrix?: ArrayLike<number>;
  inverseViewMatrix?: ArrayLike<number>;
  inverseViewProjectionMatrix?: ArrayLike<number>;
  previousViewProjectionMatrix?: ArrayLike<number>;
  inverseRasterViewProjectionMatrix?: ArrayLike<number>;
  previousRasterViewProjectionMatrix?: ArrayLike<number>;
  nearPlane?: number;
  farPlane?: number;
  projection?: 'perspective' | 'orthographic';
  currentJitter?: readonly [number, number];
  previousJitter?: readonly [number, number];
  directionalLightDirectionView?: readonly [number, number, number];
  directionalLightColor?: readonly [number, number, number];
  clusteredLighting?: SceneVolumeLightBindings;
  frameIndex?: number;
  timeSeconds?: number;
  deltaTimeSeconds?: number;
  resetHistory?: boolean;
};

type SceneEffectBindings = NonNullable<ShaderPassRendererRenderOptions['bindings']>;
type SceneEffectUniforms = NonNullable<ShaderPassRendererRenderOptions['uniforms']>;

const FRAME_KEYS: readonly (keyof SceneEffectsFrame)[] = [
  'depthTexture',
  'normalTexture',
  'velocityTexture',
  'taaVelocityTexture',
  'projectionMatrix',
  'inverseProjectionMatrix',
  'inverseViewMatrix',
  'inverseViewProjectionMatrix',
  'previousViewProjectionMatrix',
  'inverseRasterViewProjectionMatrix',
  'previousRasterViewProjectionMatrix',
  'nearPlane',
  'farPlane',
  'projection',
  'currentJitter',
  'previousJitter',
  'directionalLightDirectionView',
  'directionalLightColor',
  SCENE_EFFECT.CLUSTERED_LIGHTING,
  'frameIndex',
  'timeSeconds',
  'deltaTimeSeconds',
  'resetHistory'
];

const WEBGPU_REQUIRED = 'Requires the WebGPU backend.';
const NORMAL_TEXTURE_FORMATS: readonly string[] = [
  GPU_TEXTURE_FORMAT.RGBA8UNORM,
  GPU_TEXTURE_FORMAT.RGBA16FLOAT,
  'rgba32float'
];

/**
 * Owns optional upstream scene effects and temporal targets. The device and frame resources
 * remain borrowed. Output remains valid until the next render, resize, reset, or destroy.
 * WebGL preserves the source and reports each requested effect as unavailable.
 */
export class SceneEffects {
  // Borrowed dependency; its creator remains responsible for device destruction.
  readonly device: Device;

  // Frozen configuration and capability snapshots; configuration remains publicly replaceable.
  configuration: NormalizedSceneEffectsOptions;
  readonly capabilities: ReturnType<typeof createCapabilityReport>;

  // Owned stage renderers, released in reverse submission order.
  renderer: ShaderPassRenderer | null;
  temporalRenderer: ShaderPassRenderer | null;
  finishingRenderer: ShaderPassRenderer | null;

  // Target dimensions and temporal state for the last successful submission.
  width: number;
  height: number;
  frameIndex: number;
  depthConvention: string | null;
  destroyed: boolean;

  /**
   * Create enabled WebGPU stages, releasing completed stages if initialization fails.
   * @param device Borrowed rendering device; this owner never destroys it.
   * @param options Effect configuration copied and normalized for this instance.
   */
  constructor(device: Device, options: SceneEffectsOptions = {}) {
    const requested = normalizeSceneEffectsOptions(options);
    const supported = device.type === GPU_BACKEND.WEBGPU;
    const enabled = SCENE_EFFECT_NAMES.some(name => Boolean(requested[name]));
    const colorFormat = supported && enabled ? resolveColorFormat(device, requested.colorFormat) : null;
    this.device = device;

    this.configuration = Object.freeze({
      ...requested,
      colorFormat
    });
    this.capabilities = createCapabilityReport(device.type, this.configuration, supported);

    this.renderer = null;
    this.temporalRenderer = null;
    this.finishingRenderer = null;

    this.width = 0;
    this.height = 0;
    this.frameIndex = 0;
    this.depthConvention = null;
    this.destroyed = false;

    if(!supported || !enabled || !colorFormat) {
      return;
    }

    const pipelines = createSceneEffectPipelines(this.configuration);
    validateSceneEffectFormats(device, pipelines);

    // Each stage has distinct velocity bindings. Successful stages are released if a later stage fails.
    try {
      if(pipelines.lighting.length) {
        this.renderer = new ShaderPassRenderer(device, {
          shaderPasses: pipelines.lighting,
          colorFormat
        });
      }
      if(pipelines.temporal.length) {
        this.temporalRenderer = new ShaderPassRenderer(device, {
          shaderPasses: pipelines.temporal,
          colorFormat
        });
      }
      if(pipelines.finishing.length) {
        this.finishingRenderer = new ShaderPassRenderer(device, {
          shaderPasses: pipelines.finishing,
          colorFormat
        });
      }
    } catch(error) {
      this.destroy();
      throw error;
    }
  }

  /**
   * Validate every attachment before resizing or mutating history, then execute scene-linear passes.
   * A failed stage invalidates history for the entire frame and rethrows the original error.
   * @param sourceTexture Borrowed scene color texture matching the configured color format.
   * @param frame Borrowed attachments, camera matrices, and timing in seconds for this submission.
   * @returns Borrowed output owned by this instance, valid until the next render, resize, reset, or destroy.
   * Callers must not destroy it. When disabled, returns the caller's borrowed source unchanged.
   */
  render(sourceTexture: Texture, frame: SceneEffectsFrame = {}): Texture {
    this.#assertAlive();
    if(!this.renderer && !this.temporalRenderer && !this.finishingRenderer) {
      return sourceTexture;
    }

    const options = createRenderOptions(
      this.device,
      this.configuration,
      sourceTexture,
      frame,
      this.frameIndex
    );
    this.resize(sourceTexture.width, sourceTexture.height);

    const depthConvention = `${frame.projection ?? 'perspective'}:${frame.nearPlane}:${frame.farPlane}`;
    if(this.depthConvention !== null && this.depthConvention !== depthConvention) {
      this.resetHistory();
    }

    let result = sourceTexture;
    try {
      for(const renderer of [this.renderer, this.temporalRenderer, this.finishingRenderer]) {
        if(!renderer) {
          continue;
        }

        const bindings = {
          ...options.bindings
        };
        if(renderer !== this.renderer && frame.taaVelocityTexture) {
          bindings.velocityTexture = frame.taaVelocityTexture;
        }
        const output = renderer.renderToTexture({
          ...options,
          sourceTexture: result,
          bindings
        });
        if(!output) {
          throw new Error('Scene effects could not render an unavailable source texture.');
        }
        result = output;
      }
    } catch(error) {
      // A later stage may fail after an earlier stage committed history. Reject the whole frame.
      this.resetHistory();
      throw error;
    }
    this.depthConvention = depthConvention;
    this.frameIndex+= 1;

    return result;
  }

  /**
   * Resize owned targets and invalidate history when drawing-buffer dimensions change.
   * @param width Positive integer drawing-buffer width in pixels.
   * @param height Positive integer drawing-buffer height in pixels.
   */
  resize(width: number, height: number): void {
    this.#assertAlive();
    assertInteger(width, 1, Infinity, 'width');
    assertInteger(height, 1, Infinity, 'height');
    if(width === this.width && height === this.height) {
      return;
    }
    for(const renderer of [this.renderer, this.temporalRenderer, this.finishingRenderer]) {
      renderer?.resize([width, height]);
    }
    this.resetHistory();
    this.width = width;
    this.height = height;
  }

  /**
   * Invalidate owned history for camera cuts, visibility changes, or resumed rendering.
   * Throws after destruction; borrowed frame resources remain alive.
   */
  resetHistory(): void {
    this.#assertAlive();
    for(const renderer of [this.renderer, this.temporalRenderer, this.finishingRenderer]) {
      renderer?.resetHistory();
    }
    this.depthConvention = null;
  }

  /**
   * Release owned stages in reverse submission order; repeated destruction is harmless.
   * Borrowed source, attachments, buffers, and device remain alive.
   */
  destroy(): void {
    if(this.destroyed) {
      return;
    }
    this.destroyed = true;
    for(const renderer of [this.finishingRenderer, this.temporalRenderer, this.renderer]) {
      renderer?.destroy();
    }
    this.renderer = null;
    this.temporalRenderer = null;
    this.finishingRenderer = null;

    this.width = 0;
    this.height = 0;
  }

  /**
   * Reject operations after owned rendering resources have been destroyed.
   */
  #assertAlive(): void {
    if(this.destroyed) {
      throw new Error('SceneEffects has been destroyed.');
    }
  }
}

/**
 * Prepare a frame without changing owned targets or temporal history.
 * Stages retain validation order so the first reported input error remains stable.
 * @param device Borrowed device owning all submitted GPU resources.
 * @param configuration Normalized effect settings.
 * @param sourceTexture Borrowed scene-linear color texture.
 * @param frame Borrowed frame inputs; never mutated.
 * @param frameIndex Successful submission count used when the caller omits a frame index.
 * @returns New bindings and uniforms borrowing frame resources.
 */
function createRenderOptions(
  device: Device,
  configuration: NormalizedSceneEffectsOptions,
  sourceTexture: Texture,
  frame: SceneEffectsFrame,
  frameIndex: number): ShaderPassRendererRenderOptions {
  validateFrameInputs(device, configuration, sourceTexture, frame);

  const bindings = createAttachmentBindings(device, configuration, sourceTexture, frame);
  const uniforms: SceneEffectUniforms = {};
  configureScreenSpaceUniforms(configuration, frame, frameIndex, uniforms);
  configureClusteredLighting(device, configuration, frame, bindings, uniforms);
  configureTemporalUniforms(configuration, frame, uniforms);
  configureFinishingUniforms(configuration, frame, uniforms);

  return {
    sourceTexture,
    bindings,
    uniforms,
    resetHistory: frame.resetHistory ?? false
  };
}

/**
 * Validate source ownership and shared frame metadata before effect-specific inputs.
 * @param device Borrowed device expected to own the source texture.
 * @param configuration Normalized settings supplying the required color format.
 * @param sourceTexture Borrowed scene color texture.
 * @param frame Borrowed timing, projection, jitter, and history settings.
 */
function validateFrameInputs(
  device: Device,
  configuration: NormalizedSceneEffectsOptions,
  sourceTexture: Texture,
  frame: SceneEffectsFrame): void {
  assertTexture(sourceTexture, device, 'sourceTexture');
  if(sourceTexture.format !== configuration.colorFormat) {
    throw new Error(`sourceTexture format must match the scene effects ${configuration.colorFormat} color format.`);
  }
  assertRecord(frame, 'Scene effects frame');
  assertKnownKeys(frame, FRAME_KEYS, 'Scene effects frame');
  if(frame.projection !== undefined && frame.projection !== 'perspective' && frame.projection !== 'orthographic') {
    throw new TypeError('frame.projection must be perspective or orthographic.');
  }
  for(const key of ['frameIndex', 'timeSeconds', 'deltaTimeSeconds'] as const) {
    if(frame[key] !== undefined) {
      assertRange(frame[key], 0, Infinity, `frame.${key}`);
    }
  }
  if(frame.frameIndex !== undefined) {
    assertInteger(frame.frameIndex, 0, Infinity, 'frame.frameIndex');
  }
  for(const key of ['currentJitter', 'previousJitter'] as const) {
    if(frame[key] !== undefined) {
      assertVector(frame[key], 2, `frame.${key}`);
    }
  }
  if(frame.resetHistory !== undefined && typeof frame.resetHistory !== 'boolean') {
    throw new TypeError('frame.resetHistory must be a boolean.');
  }
}

/**
 * Validate required attachments and collect borrowed textures for shader bindings.
 * @param device Borrowed device expected to own every attachment.
 * @param configuration Normalized settings determining attachment requirements.
 * @param sourceTexture Borrowed color texture defining attachment dimensions.
 * @param frame Borrowed depth, normal, and velocity textures.
 * @returns New bindings referencing validated textures without transferring ownership.
 */
function createAttachmentBindings(
  device: Device,
  configuration: NormalizedSceneEffectsOptions,
  sourceTexture: Texture,
  frame: SceneEffectsFrame): SceneEffectBindings {
  const requirements = getSceneEffectsRequirements(configuration);
  const bindings: SceneEffectBindings = {};
  if(requirements.depthTexture) {
    assertTexture(frame.depthTexture, device, 'frame.depthTexture', sourceTexture);
    if(!frame.depthTexture.format.startsWith('depth')) {
      throw new Error('frame.depthTexture must use a depth texture format.');
    }
    bindings.depthTexture = frame.depthTexture;
  }
  if(requirements.normalTexture) {
    assertTexture(frame.normalTexture, device, 'frame.normalTexture', sourceTexture);
    if(!NORMAL_TEXTURE_FORMATS.includes(frame.normalTexture.format)) {
      throw new Error('frame.normalTexture must pack encoded view-space RGB normals and alpha roughness in RGBA.');
    }
    bindings.normalTexture = frame.normalTexture;
  }
  if(requirements.velocityTexture) {
    assertVelocity(frame.velocityTexture, device, 'frame.velocityTexture', sourceTexture);
    bindings.velocityTexture = frame.velocityTexture;
  }
  if(requirements.taaVelocityTexture) {
    assertVelocity(frame.taaVelocityTexture, device, 'frame.taaVelocityTexture', sourceTexture);
  }

  return bindings;
}

/**
 * Validate depth reconstruction and prepare screen-space lighting and fog uniforms.
 * @param configuration Normalized settings for enabled passes.
 * @param frame Borrowed camera matrices, clip planes, and timing in seconds.
 * @param frameIndex Successful submission count used as the sampling fallback.
 * @param uniforms New frame uniforms to populate; camera matrices remain borrowed.
 */
function configureScreenSpaceUniforms(
  configuration: NormalizedSceneEffectsOptions,
  frame: SceneEffectsFrame,
  frameIndex: number,
  uniforms: SceneEffectUniforms): void {
  if(configuration.ssao || configuration.clusteredLighting) {
    assertRange(frame.nearPlane, Number.MIN_VALUE, Infinity, 'frame.nearPlane');
    assertRange(frame.farPlane, Number.MIN_VALUE, Infinity, 'frame.farPlane');
    if(frame.farPlane <= frame.nearPlane) {
      throw new RangeError('frame.farPlane must be greater than frame.nearPlane.');
    }
  }

  if(configuration.ssao) {
    if(frame.projection === 'orthographic') {
      throw new Error('SSAO requires perspective projection depth reconstruction.');
    }
    uniforms.ssaoEvaluate = {
      ...selectUniforms(configuration.ssao, ['radius', 'bias', 'intensity']),
      nearPlane: frame.nearPlane,
      farPlane: frame.farPlane
    };
  }
  const matrices = {
    projectionMatrix: frame.projectionMatrix,
    inverseProjectionMatrix: frame.inverseProjectionMatrix
  };

  if(configuration.gtao || configuration.ssgi || configuration.ssr || configuration.clusteredLighting) {
    assertMatrix(frame.projectionMatrix, 'frame.projectionMatrix');
    assertMatrix(frame.inverseProjectionMatrix, 'frame.inverseProjectionMatrix');
  }

  if(configuration.gtao) {
    uniforms.gtaoEvaluate = {
      ...selectUniforms(configuration.gtao, ['radius', 'bias', 'intensity']),
      ...matrices,
      frameIndex: frame.frameIndex ?? frameIndex
    };
    uniforms.gtaoTemporal = {
      ...selectUniforms(configuration.gtao, ['historyWeight', 'depthThreshold']),
      inverseProjectionMatrix: frame.inverseProjectionMatrix
    };
    uniforms.gtaoComposite = {
      strength: configuration.gtao.strength
    };
  }

  if(configuration.ssgi) {
    uniforms.ssgiTrace = {
      ...selectUniforms(configuration.ssgi, ['radius', 'thickness', 'intensity', 'rayCount', 'stepCount']),
      ...matrices,
      frameIndex: frame.frameIndex ?? frameIndex
    };
    uniforms.ssgiTemporal = {
      ...selectUniforms(configuration.ssgi, ['historyWeight', 'depthThreshold']),
      inverseProjectionMatrix: frame.inverseProjectionMatrix
    };
    uniforms.ssgiComposite = {
      strength: configuration.ssgi.strength
    };
  }

  if(configuration.ssr) {
    uniforms.ssrTrace = {
      ...selectUniforms(configuration.ssr, ['intensity', 'maxDistance', 'thickness', 'sampleCount', 'maxRoughness']),
      ...matrices,
      frameIndex: frame.frameIndex ?? frameIndex
    };
    uniforms.ssrTemporal = {
      ...selectUniforms(configuration.ssr, ['historyWeight', 'depthThreshold']),
      inverseProjectionMatrix: frame.inverseProjectionMatrix
    };
    uniforms.ssrSpatial = {
      inverseProjectionMatrix: frame.inverseProjectionMatrix
    };
    uniforms.ssrComposite = {
      strength: configuration.ssr.strength,
      inverseProjectionMatrix: frame.inverseProjectionMatrix
    };
  }

  if(configuration.outlines) {
    uniforms.screenSpaceOutline = selectUniforms(configuration.outlines, ['color', 'thickness', 'depthThreshold', 'normalThreshold']);
  }

  if(configuration.heightFog) {
    uniforms.volumetricFog = {
      ...configuration.heightFog,
      historyWeight: 0,
      time: frame.timeSeconds ?? 0
    };
  }
}

/**
 * Validate clustered camera/light resources before preparing volume-light shader inputs.
 * @param device Borrowed device expected to own clustered light buffers.
 * @param configuration Normalized clustered-lighting settings.
 * @param frame Borrowed camera snapshot, light buffers, and directional-light values.
 * @param bindings New frame bindings to populate with borrowed light buffers.
 * @param uniforms New frame uniforms to populate without modifying frame inputs.
 */
function configureClusteredLighting(
  device: Device,
  configuration: NormalizedSceneEffectsOptions,
  frame: SceneEffectsFrame,
  bindings: SceneEffectBindings,
  uniforms: SceneEffectUniforms): void {
  if(!configuration.clusteredLighting) {
    return;
  }

  assertMatrix(frame.inverseViewMatrix, 'frame.inverseViewMatrix');
  const matrices = {
    projectionMatrix: frame.projectionMatrix,
    inverseProjectionMatrix: frame.inverseProjectionMatrix
  };
  const inverseRaster = frame.inverseRasterViewProjectionMatrix ?? frame.inverseViewProjectionMatrix;
  const previousRaster = frame.previousRasterViewProjectionMatrix ?? frame.previousViewProjectionMatrix;
  assertMatrix(inverseRaster, 'frame.inverseRasterViewProjectionMatrix');
  assertMatrix(previousRaster, 'frame.previousRasterViewProjectionMatrix');
  if(frame.currentJitter?.some(value => value !== 0) &&
    (!frame.inverseRasterViewProjectionMatrix || !frame.previousRasterViewProjectionMatrix)) {
    throw new Error('Jittered clustered lighting requires raster view-projection matrices.');
  }
  validateClusteredBindings(frame.clusteredLighting, device);
  const clusters = frame.clusteredLighting;
  Object.assign(bindings, {
    pointLights: clusters.pointLights,
    clusterLightCounts: clusters.clusterLightCounts,
    clusterLightIndices: clusters.clusterLightIndices
  });
  const direction = frame.directionalLightDirectionView ?? [0, 1, 0];
  const color = frame.directionalLightColor ?? [1, 1, 1];
  assertVector(direction, 3, 'frame.directionalLightDirectionView');
  assertVector(color, 3, 'frame.directionalLightColor');
  if(Math.hypot(...direction) < 0.00001) {
    throw new RangeError('frame.directionalLightDirectionView must be nonzero.');
  }
  for(const channel of color) {
    assertRange(channel, 0, Infinity, 'frame.directionalLightColor');
  }
  uniforms.clusteredVolumetricTrace = {
    ...selectUniforms(configuration.clusteredLighting, [
      'fogColor',
      'density',
      'heightFalloff',
      'fogHeight',
      'anisotropy',
      'directionalIntensity',
      'pointLightIntensity',
      'maxDistance',
      'sampleCount',
      'shadowStrength'
    ]),
    ...matrices,
    inverseViewMatrix: frame.inverseViewMatrix,
    clusterCountX: clusters.clusterCountX,
    clusterCountY: clusters.clusterCountY,
    clusterCountZ: clusters.clusterCountZ,
    maxLightsPerCluster: clusters.maxLightsPerCluster,
    pointLightCount: clusters.pointLightCount,
    clusterNearPlane: frame.nearPlane,
    clusterFarPlane: frame.farPlane,
    directionalLightDirectionView: direction,
    directionalLightColor: color
  };
  uniforms.clusteredVolumetricTemporal = {
    historyWeight: configuration.clusteredLighting.historyWeight,
    depthThreshold: configuration.clusteredLighting.depthThreshold,
    inverseProjectionMatrix: frame.inverseProjectionMatrix,
    inverseViewProjectionMatrix: inverseRaster,
    previousViewProjectionMatrix: previousRaster
  };
  uniforms.clusteredVolumetricDepthHistoryCopy = {
    inverseProjectionMatrix: frame.inverseProjectionMatrix
  };
  uniforms.clusteredVolumetricComposite = {
    strength: configuration.clusteredLighting.strength
  };
}

/**
 * Validate temporal jitter and select camera or velocity reprojection uniforms.
 * @param configuration Normalized temporal antialiasing settings.
 * @param frame Borrowed current/previous camera matrices and UV jitter.
 * @param uniforms New frame uniforms to populate with borrowed camera inputs.
 */
function configureTemporalUniforms(
  configuration: NormalizedSceneEffectsOptions,
  frame: SceneEffectsFrame,
  uniforms: SceneEffectUniforms): void {
  if(configuration.taa) {
    assertVector(frame.currentJitter, 2, 'frame.currentJitter');
    assertVector(frame.previousJitter, 2, 'frame.previousJitter');
    const temporal = {
      historyWeight: configuration.taa.historyWeight,
      depthThreshold: configuration.taa.depthThreshold,
      currentJitter: frame.currentJitter,
      previousJitter: frame.previousJitter
    };
    if(configuration.taa.reprojection === SCENE_REPROJECTION.CAMERA) {
      assertMatrix(frame.inverseViewProjectionMatrix, 'frame.inverseViewProjectionMatrix');
      assertMatrix(frame.previousViewProjectionMatrix, 'frame.previousViewProjectionMatrix');
      uniforms.cameraReprojectionTaaResolve = {
        ...temporal,
        inverseViewProjectionMatrix: frame.inverseViewProjectionMatrix,
        previousViewProjectionMatrix: frame.previousViewProjectionMatrix
      };
    } else {
      uniforms.taaResolve = temporal;
    }
  }
}

/**
 * Prepare motion blur and exposure adaptation after lighting and reprojection inputs.
 * @param configuration Normalized finishing-pass settings.
 * @param frame Borrowed timing in seconds; exposure adaptation clamps long frame intervals.
 * @param uniforms New frame uniforms to populate with copied effect settings.
 */
function configureFinishingUniforms(
  configuration: NormalizedSceneEffectsOptions,
  frame: SceneEffectsFrame,
  uniforms: SceneEffectUniforms): void {
  if(configuration.motionBlur) {
    uniforms.motionBlur = {
      ...configuration.motionBlur
    };
  }

  if(configuration.adaptiveExposure) {
    uniforms.hdrAutoExposureAdapt = {
      ...selectUniforms(configuration.adaptiveExposure, [
        'keyValue',
        'minimumExposure',
        'maximumExposure',
        'brightenSpeed',
        'darkenSpeed'
      ]),
      deltaTime: Math.min(frame.deltaTimeSeconds ?? (1/60), 0.25),
      enabled: 1
    };
    uniforms.hdrAutoExposureApply = {
      enabled: 1
    };
  }
}

/**
 * Copy only uniforms consumed by a shader pass, preserving settings entry order.
 * @param settings Normalized effect configuration; nested values remain borrowed.
 * @param keys Uniform names consumed by the destination pass.
 * @returns A new record containing the selected configuration entries.
 */
function selectUniforms(settings: object, keys: readonly string[]): Record<string, unknown> {
  return Object.fromEntries(Object.entries(settings).filter(([key]) => keys.includes(key)));
}

/**
 * Resolve the requested scene color format against render/filter/blend capabilities.
 * @param device Borrowed device queried for floating-point texture support.
 * @param requested Explicit color format, or null to choose the supported default.
 * @returns The requested format or the highest supported default; unsupported explicit HDR throws.
 */
function resolveColorFormat(
  device: Device,
  requested: typeof GPU_TEXTURE_FORMAT.RGBA16FLOAT | typeof GPU_TEXTURE_FORMAT.RGBA8UNORM | null) {
  const capabilities = device.getTextureFormatCapabilities(GPU_TEXTURE_FORMAT.RGBA16FLOAT);
  const supportsHdr = capabilities.create && capabilities.render && capabilities.filter && capabilities.blend;
  if(requested === GPU_TEXTURE_FORMAT.RGBA16FLOAT && !supportsHdr) {
    throw new Error('Scene effects colorFormat rgba16float is not supported by this device.');
  }
  return requested ?? (supportsHdr ? GPU_TEXTURE_FORMAT.RGBA16FLOAT : GPU_TEXTURE_FORMAT.RGBA8UNORM);
}

/**
 * Describe availability without creating effect resources.
 * @param backend Device backend name included in the immutable report.
 * @param configuration Normalized settings identifying requested effects.
 * @param supported Whether this backend can execute advanced scene effects.
 * @returns A frozen report with frozen per-effect capability records.
 */
function createCapabilityReport(
  backend: string,
  configuration: NormalizedSceneEffectsOptions,
  supported: boolean) {
  const effects = Object.fromEntries(SCENE_EFFECT_NAMES.map(name => {
    const requested = Boolean(configuration[name]);
    return [name, Object.freeze({
      requested,
      available: requested && supported,
      reason: requested && !supported ? WEBGPU_REQUIRED : null
    })];
  }));
  return Object.freeze({
    backend,
    available: supported,
    colorFormat: supported ? configuration.colorFormat : null,
    effects: Object.freeze(effects)
  });
}

/**
 * Validate a finite integer within an inclusive range.
 * @param value Input to validate without mutation.
 * @param minimum Inclusive lower bound.
 * @param maximum Inclusive upper bound.
 * @param name Input path used in validation errors.
 */
function assertInteger(
  value: unknown,
  minimum: number,
  maximum: number,
  name: string): asserts value is number {
  assertRange(value, minimum, maximum, name);
  if(!Number.isInteger(value)) {
    throw new RangeError(`${name} must be an integer.`);
  }
}

/**
 * Validate a borrowed column-major 4x4 matrix with finite components.
 * @param value Matrix input to validate without mutation.
 * @param name Input path used in validation errors.
 */
function assertMatrix(value: unknown, name: string): asserts value is ArrayLike<number> {
  if(!value || typeof value !== 'object' || !('length' in value) || value.length !== 16) {
    throw new TypeError(`${name} must be a column-major 16-number matrix.`);
  }
  for(let index = 0; index < 16; index+= 1) {
    assertRange((value as ArrayLike<unknown>)[index], -Infinity, Infinity, name);
  }
}

/**
 * Validate borrowed texture lifetime, device ownership, and optional attachment dimensions.
 * @param value Texture input to validate without taking ownership.
 * @param device Expected owning device.
 * @param name Input path used in validation errors.
 * @param source Optional source defining the required pixel dimensions.
 */
function assertTexture(
  value: unknown,
  device: Device,
  name: string,
  source?: Texture): asserts value is Texture {
  const texture = value as Partial<Texture> | null;
  if(!texture || !Number.isInteger(texture.width) || !Number.isInteger(texture.height) || !texture.format || texture.destroyed) {
    throw new TypeError(`${name} must be a live GPU texture.`);
  }
  if(texture.device !== device) {
    throw new Error(`${name} must belong to the SceneEffects device.`);
  }
  if(source && (texture.width !== source.width || texture.height !== source.height)) {
    throw new Error(`${name} dimensions must match sourceTexture.`);
  }
}

/**
 * Validate a borrowed signed screen-space motion attachment.
 * @param value Velocity texture input to validate without mutation.
 * @param device Expected owning device.
 * @param name Input path used in validation errors.
 * @param source Borrowed color texture defining the required pixel dimensions.
 */
function assertVelocity(
  value: unknown,
  device: Device,
  name: string,
  source: Texture): asserts value is Texture {
  assertTexture(value, device, name, source);
  if(value.format !== GPU_TEXTURE_FORMAT.RG16FLOAT && value.format !== GPU_TEXTURE_FORMAT.RGBA16FLOAT) {
    throw new Error(`${name} must use rg16float or rgba16float for signed screen-space motion.`);
  }
}

/**
 * Validate clustered-light dimensions and the lifetime/capacity of borrowed GPU buffers.
 * @param value Clustered bindings to validate without mutation or ownership transfer.
 * @param device Expected owning device for every light buffer.
 */
function validateClusteredBindings(value: unknown, device: Device): asserts value is SceneVolumeLightBindings {
  assertRecord(value, 'frame.clusteredLighting');
  assertKnownKeys(value, [
    'pointLights',
    'clusterLightCounts',
    'clusterLightIndices',
    'clusterCountX',
    'clusterCountY',
    'clusterCountZ',
    'maxLightsPerCluster',
    'pointLightCount'
  ], 'frame.clusteredLighting');
  for(const key of ['clusterCountX', 'clusterCountY', 'clusterCountZ'] as const) {
    assertInteger(value[key], 1, key === 'clusterCountZ' ? 16 : 32, `frame.clusteredLighting.${key}`);
  }
  assertInteger(value.maxLightsPerCluster, 1, 8, 'frame.clusteredLighting.maxLightsPerCluster');
  assertInteger(value.pointLightCount, 0, 32, 'frame.clusteredLighting.pointLightCount');
  const count = (value.clusterCountX as number)*(value.clusterCountY as number)*(value.clusterCountZ as number);
  const sizes = {
    pointLights: Math.max(1, value.pointLightCount as number)*32,
    clusterLightCounts: count*4,
    clusterLightIndices: count*(value.maxLightsPerCluster as number)*4
  };
  for(const [key, minimum] of Object.entries(sizes)) {
    const buffer = value[key] as {
      device?: Device;
      byteLength?: number;
      destroyed?: boolean;
    } | null;
    if(!buffer || buffer.device !== device || buffer.destroyed || typeof buffer.byteLength !== 'number' || buffer.byteLength < minimum) {
      throw new Error(`frame.clusteredLighting.${key} must be a live device buffer with at least ${minimum} bytes.`);
    }
  }
}
