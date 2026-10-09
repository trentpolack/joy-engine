// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { GPU_BACKEND } from '../gpu/constants.ts';
import type { RendererDiagnosticOptions, RendererDiagnostics } from '../gpu/gpu-diagnostics.ts';
import type { PostProcessingConfig } from '../postprocessor/config.ts';
import type { GpuDeviceOptions } from '../gpu/gpu-device.ts';
import type { EnvironmentFrame } from '../../environment/environment-system.ts';
import type { PointLight } from '../pbr/point-lights.ts';
import type { Color, Vector2 } from '../../core/types.ts';
import type { RenderScene } from './render-scene.ts';
import type {SceneEffectsOptions, SceneEffectsFrame} from './scene-effects.ts';
import type {SceneTemporalSnapshot} from './scene-temporal.ts';
import type {SceneAttachmentFrame} from './scene-effect-attachments.ts';
import {normalizeSceneEffectsOptions} from './scene-effect-config.ts';
import {prepareSceneTemporalFrame} from './scene-temporal.ts';
import {multiplyCameraMatrices, sceneCameraJitter, jitterProjection} from './scene-camera-frame.ts';
import type { WorldRay } from '../camera/projection.ts';
import { GpuTriangleRenderer } from '../renderers/gpu-triangle-renderer.ts';
import { OrbitCamera } from '../camera/orbit-camera.ts';
import { projectToViewport, screenRay, intersectRayPlane, invertMatrix } from '../camera/projection.ts';
import { LIT_TRIANGLE_SHADERS, POINT_LIGHT_UNIFORM_BYTES, createPointLightUniforms } from '../pbr/point-lights.ts';
import { createAtmosphereRenderUniforms } from '../environment/atmosphere-rendering.ts';
import { gpuBackendFromQuery } from '../gpu/gpu-device.ts';
import { prepareSceneFrame } from './prepare-frame.ts';
import wgsl from '../../../shaders/rendering/scene/scene.wgsl?raw';
import vertex from '../../../shaders/rendering/scene/scene.vert.glsl?raw';
import fragment from '../../../shaders/rendering/scene/scene.frag.glsl?raw';

export interface SceneRendererOptions {
  camera?: OrbitCamera;
  atmosphere?: boolean;
  pixelRatio?: number;
  postProcessing?: Partial<PostProcessingConfig> | false;
  device?: GpuDeviceOptions;
  /** Individual WebGPU effect opt-ins. Opaque scene effects precede transparency and HDR display processing. */
  sceneEffects?: SceneEffectsOptions;
}
export interface SceneFrameOptions {
  environment?: EnvironmentFrame;
  lights?: readonly PointLight[];
  ambient?: number;
  clearColor?: Color;
  timeSeconds?: number;
  opaqueTriangles?: readonly number[];
  transparentTriangles?: readonly number[];
  culling?: boolean;
  /** Seconds since the preceding rendered frame, for exposure adaptation. */
  deltaTimeSeconds?: number;
  /** Explicit camera cut, teleport, procedural topology change or other discontinuity. */
  resetHistory?: boolean;
}

/**
 * Owns GPU frame preparation and disposal; borrows camera, scenes and transient geometry.
 * Standard mode is flat-lit vertex color for opaque meshes and unlit transparency.
 * Authored PBR channels remain on assets; this adapter does not interpret their materials.
 */
export class SceneRenderer {
  declare canvas: HTMLCanvasElement;
  declare camera: OrbitCamera;
  declare options: SceneRendererOptions;
  private declare renderer: GpuTriangleRenderer | null;
  private declare initialization: Promise<void> | null;
  declare destroyed: boolean;
  declare width: number;
  declare height: number;
  declare pixelRatio: number;
  private temporalSnapshot: SceneTemporalSnapshot | null = null;
  private previousCamera: {camera: OrbitCamera; eye: readonly number[]; yaw: number; pitch: number;
    projection: Float32Array<ArrayBuffer>; matrix: Float32Array<ArrayBuffer>; rasterMatrix: Float32Array<ArrayBuffer>; jitter: [number, number]; time: number} | null = null;
  private frameIndex = 0;
  declare stats: { visible: number; culled: number; };

  /**
   * @param canvas
   * @param [options]
   */
  constructor(canvas: HTMLCanvasElement, options: SceneRendererOptions = {}) {
    if(options.sceneEffects) {
      normalizeSceneEffectsOptions(options.sceneEffects);
      if(options.postProcessing === false) {
        throw new Error('Scene effects require the offscreen HDR/display path.');
      }
    }
    this.canvas = canvas;
    this.camera = options.camera ?? new OrbitCamera();
    this.options = {...options, sceneEffects: options.sceneEffects ? structuredClone(options.sceneEffects) : undefined};

    /** @private */
    this.renderer = null;

    /** @private */
    this.initialization = null;

    this.destroyed = false;
    this.width = 0;
    this.height = 0;
    this.pixelRatio = 0;
    this.stats = {visible: 0, culled: 0};
  }

  /** Initialize once; destruction during startup releases the late GPU result. */
  async initialize() {
    if(this.destroyed) {
      throw new Error('SceneRenderer is destroyed.');
    }
    this.initialization ??= this.initializeRenderer();
    await this.initialization;
  }

  get ready() {
    return((this.renderer) !== null && !this.destroyed);
  }
  get backend() {
    return(this.renderer?.backend ?? null);
  }
  get colorFormat() {
    return(this.renderer?.postprocessor?.format ?? null);
  }

  /** Read-only display settings snapshot for opt-in controls; null on a direct canvas path. */
  get postProcessingConfig(): Readonly<PostProcessingConfig> | null {
    return this.renderer?.postProcessing ? Object.freeze({...this.renderer.postProcessing}) : null;
  }

  /** Backend requirements and requested effect availability; null before initialization. */
  get sceneEffectsCapabilities() {
    return this.renderer?.sceneEffects?.capabilities ?? null;
  }

  get sceneVolumeStats() {
    return this.renderer?.sceneVolumeStats ?? null;
  }

  /** Replace individual opt-ins atomically; preserves the working graph if validation fails. */
  setSceneEffectsConfig(config: SceneEffectsOptions) {
    if(!this.ready || !this.renderer) {
      throw new Error('Initialize SceneRenderer before updating scene effects.');
    }
    normalizeSceneEffectsOptions(config);
    this.renderer.setSceneEffectsConfig(config);
    this.options.sceneEffects = structuredClone(config);
    this.resetHistory();
  }

  /** Call after teleports, discontinuous animation or content edits that retain instance identities. */
  resetHistory() {
    this.temporalSnapshot = null;
    this.previousCamera = null;
    this.frameIndex = 0;
    this.renderer?.resetSceneHistory();
  }

  /**
   * Copy requested global luma counters and this scene's last culling counts.
   * No sampling runs unless called; returns null before initialization or after disposal.
   */
  getDiagnostics(options: RendererDiagnosticOptions = {}): RendererDiagnostics | null {
    const snapshot = this.renderer?.getDiagnostics(options) ?? null;
    if(snapshot) {
      snapshot.scene = {...this.stats};
    }
    return snapshot;
  }

  /**
   * Render in seconds. Canvas dimensions are CSS pixels; backing density never changes view.
   * Atmosphere must be enabled at construction. A borrowed environment frame drives
   * both background and directional/skylight irradiance; omitting it hides the sky.
   * Transient triangles are a borrowed escape hatch for bespoke procedural feedback/particles.
   * @param scene
   * @param [frame]
   * @returns Whether the frame was rendered; false if the renderer is not ready or the canvas is hidden.
   */
  render(scene: RenderScene, frame: SceneFrameOptions = {}): boolean {
    if(!this.ready || !this.renderer) {
      return false;
    }
    if(!this.resize()) {
      this.resetHistory();
      return false;
    }

    const webgpu = this.backend === GPU_BACKEND.WEBGPU;
    const effectsEnabled = webgpu && Boolean(this.options.sceneEffects);
    const viewMatrix = this.camera.viewMatrix();
    const projection = this.camera.projectionMatrix(this.width/this.height, webgpu);
    const unjitteredMatrix = multiplyCameraMatrices(projection, viewMatrix);
    const backingWidth = Math.round(this.width*this.pixelRatio);
    const backingHeight = Math.round(this.height*this.pixelRatio);
    const time = frame.timeSeconds ?? 0;
    const eye = this.camera.basis().eye;
    const prior = this.previousCamera;
    const timeDiscontinuity = prior && ((time < prior.time) || ((time - prior.time) > 0.25));
    const cameraCut = prior && (prior.camera !== this.camera ||
      Math.hypot(...eye.map((value, axis) => value - prior.eye[axis])) > this.camera.distance*0.5 ||
      Math.abs(this.camera.yaw - prior.yaw) > 0.5 || Math.abs(this.camera.pitch - prior.pitch) > 0.5 ||
      projection.some((value, index) => value !== prior.projection[index]));
    if(effectsEnabled && (frame.resetHistory || timeDiscontinuity || cameraCut || this.temporalSnapshot?.scene !== scene)) {
      this.resetHistory();
    }
    const taa = this.options.sceneEffects?.taa;
    const jitter: [number, number] = effectsEnabled && taa ? sceneCameraJitter(this.frameIndex, backingWidth, backingHeight) : [0, 0];
    const rasterProjection = jitterProjection(projection, jitter);
    const matrix = multiplyCameraMatrices(rasterProjection, viewMatrix);
    const queues = prepareSceneFrame(scene, {backward: this.camera.basis().backward,
      viewProjection: frame.culling === false ? undefined : unjitteredMatrix, webgpu,
      opaqueTriangles: frame.opaqueTriangles, transparentTriangles: frame.transparentTriangles});
    const ambient = frame.ambient ?? (frame.environment ? 0 : 0.3);
    const uniforms = createPointLightUniforms(matrix, frame.lights ?? [], ambient, frame.environment);
    let atmosphereUniforms = null;
    if(frame.environment) {
      if(!this.options.atmosphere) {
        throw new Error('Environment frames require atmosphere at SceneRenderer creation.');
      }
      const inverse = invertMatrix(matrix);
      if(!inverse) {
        throw new Error('Atmosphere requires an invertible camera matrix.');
      }
      const eye = this.camera.basis().eye;
      atmosphereUniforms = createAtmosphereRenderUniforms(inverse, [eye[0], eye[1], eye[2]],
        [Math.round(this.width*this.pixelRatio), Math.round(this.height*this.pixelRatio)], frame.environment);
    }

    let effectsFrame: SceneEffectsFrame = {};
    let attachments: SceneAttachmentFrame | null = null;
    let temporal: ReturnType<typeof prepareSceneTemporalFrame> | null = null;
    if(effectsEnabled) {
      temporal = prepareSceneTemporalFrame(this.temporalSnapshot, {
        scene, vertices: queues.opaque, ranges: queues.opaqueRanges, viewProjection: unjitteredMatrix
      });
      const previous = this.previousCamera;
      const previousJitter = previous?.jitter ?? jitter;
      const inverseProjection = invertMatrix(rasterProjection);
      const inverseView = invertMatrix(viewMatrix);
      const inverseViewProjection = invertMatrix(unjitteredMatrix);
      const inverseRasterViewProjection = invertMatrix(matrix);
      if(!inverseProjection || !inverseView || !inverseViewProjection || !inverseRasterViewProjection) {
        throw new Error('Scene effects require invertible perspective camera matrices.');
      }
      effectsFrame = {
        nearPlane: this.camera.nearPlane, farPlane: this.camera.farPlane, projection: 'perspective',
        projectionMatrix: rasterProjection, inverseProjectionMatrix: inverseProjection,
        inverseViewMatrix: inverseView, inverseViewProjectionMatrix: inverseViewProjection,
        previousViewProjectionMatrix: previous?.matrix ?? unjitteredMatrix,
        inverseRasterViewProjectionMatrix: inverseRasterViewProjection,
        previousRasterViewProjectionMatrix: previous?.rasterMatrix ?? matrix,
        currentJitter: jitter, previousJitter,
        resetHistory: temporal.resetHistory, frameIndex: this.frameIndex,
        timeSeconds: time, deltaTimeSeconds: frame.deltaTimeSeconds ?? (previous ? Math.max(0, time - previous.time) : 0),
        directionalLightDirectionView: frame.environment ? transformDirection(viewMatrix, frame.environment.primaryLight.direction) : [0, 1, 0],
        directionalLightColor: frame.environment ? scaleLightColor(frame.environment.primaryLight.color, frame.environment.primaryLight.intensity) : [0, 0, 0]
      };
      attachments = {
        motion: {vertices: queues.opaque, previousPositions: temporal.previousPositions,
          currentViewProjection: unjitteredMatrix, previousViewProjection: temporal.previousViewProjection,
          rasterViewProjection: matrix, velocityJitterDelta: [jitter[0] - previousJitter[0], jitter[1] - previousJitter[1]],
          width: backingWidth, height: backingHeight},
        viewMatrix, surfaceRoughness: queues.surfaceRoughness, lights: frame.lights ?? []
      };
    }
    try {
      this.renderer.render(queues.opaque, uniforms, frame.clearColor ?? [0, 0, 0, 1], queues.transparent,
        time, [], null, effectsFrame, atmosphereUniforms, attachments);
    } catch(error) {
      if(effectsEnabled) {
        this.resetHistory();
      }
      throw error;
    }
    if(temporal) {
      this.temporalSnapshot = temporal.snapshot;
      this.previousCamera = {camera: this.camera, eye, yaw: this.camera.yaw, pitch: this.camera.pitch,
        projection, matrix: unjitteredMatrix, rasterMatrix: matrix, jitter, time};
      this.frameIndex+= 1;
    }
    this.stats = {visible: queues.visible, culled: queues.culled};
    return true;
  }

  /**
   * Client CSS pixels to a world ray; independent of backing pixel density.
   * @param clientX
   * @param clientY
   * @returns World ray or null if the canvas is hidden or has no area.
   */
  screenRay(clientX: number, clientY: number): WorldRay | null {
    const bounds = this.canvas.getBoundingClientRect();
    if((bounds.width <= 0) || (bounds.height <= 0)) {
      return null;
    }
    const matrix = this.camera.matrix(bounds.width/bounds.height, this.backend === GPU_BACKEND.WEBGPU);

    return(screenRay(clientX, clientY, matrix, bounds, this.backend === GPU_BACKEND.WEBGPU));
  }

  /**
   * Pick a horizontal plane at world-space height.
   * @param x
   * @param y
   * @param [height]
   * @returns World position or null if the canvas is hidden, has no area, or the ray is parallel to the plane.
   */
  pickGround(x: number, y: number, height: number = 0): readonly number[] | null {
    return(intersectRayPlane(this.screenRay(x, y), [0, 1, 0], -height));
  }

  /** 
   * World position to canvas-local CSS pixels, or null behind camera/when hidden.
   * @param position
   * @returns Canvas-local CSS pixels or null if the position is behind the camera or the canvas is hidden.
   */
  project(position: readonly number[]): Vector2 | null {
    const bounds = this.canvas.getBoundingClientRect();
    if((bounds.width <= 0) || (bounds.height <= 0)) {
      return null;
    }

    return(projectToViewport(position, this.camera.matrix(bounds.width/bounds.height, this.backend === GPU_BACKEND.WEBGPU), bounds));
  }

  /** 
   * Set postprocessing configuration. Must be called after initialization and before rendering.
   * @param config
   */
  setPostProcessingConfig(config: Partial<PostProcessingConfig>) {
    if(!this.renderer) {
      throw new Error('Initialize SceneRenderer before updating postprocessing.');
    }

    this.renderer.setPostProcessingConfig(config);
  }

  /** Idempotent. Does not destroy borrowed scene instances or camera state. */
  destroy() {
    if(this.destroyed) {
      return;
    }

    this.resetHistory();
    this.destroyed = true;
    this.renderer?.destroy();
    this.renderer = null;
  }

  /** 
   * Initialize the GPU renderer.
   * @private
   */
  private async initializeRenderer() {
    const renderer = await GpuTriangleRenderer.create(this.canvas, {
      shaderSources: {wgsl, glsl: {vertex, fragment}}, opaqueShaderSources: LIT_TRIANGLE_SHADERS,
      uniformBufferSize: POINT_LIGHT_UNIFORM_BYTES,
      atmosphere: this.options.atmosphere,
      sceneEffects: this.options.sceneEffects,
      postProcessing: this.options.postProcessing === false ? undefined : (this.options.postProcessing ?? {}),
      device: {...(this.options.device ?? {backend: gpuBackendFromQuery(globalThis.location?.search ?? '')}),
        optionalFeatures: [...(this.options.device?.optionalFeatures ?? []), 'float32-filterable']}
    });

    if(this.destroyed) {
      renderer.destroy();
      return;
    }

    this.renderer = renderer;
  }

  /** 
   * Resize the renderer to match the canvas size and pixel ratio.
   * @private
   * @returns Whether the renderer was resized; false if the canvas is hidden or has no area.
   */
  private resize(): boolean {
    const width = this.canvas.clientWidth;
    const height = this.canvas.clientHeight;
    const pixelRatio = this.options.pixelRatio ?? Math.min(2, globalThis.devicePixelRatio || 1);
    if((width <= 0) || (height <= 0)) {
      return false;
    } else if(!Number.isFinite(pixelRatio) || (pixelRatio <= 0)) {
      throw new RangeError('Scene pixel ratio must be finite and positive.');
    }

    if((width !== this.width) || (height !== this.height) || (pixelRatio !== this.pixelRatio)) {
      this.resetHistory();
      this.renderer?.resize(width, height, pixelRatio);
      this.width = width;
      this.height = height;
      this.pixelRatio = pixelRatio;
    }

    return true;
  }
}

/** Rotate a borrowed world-space light direction into the camera's view basis. */
function transformDirection(view: ArrayLike<number>, direction: readonly number[]): [number, number, number] {
  return [0, 1, 2].map(row => view[row]*direction[0] + view[row + 4]*direction[1] + view[row + 8]*direction[2]) as [number, number, number];
}

function scaleLightColor(color: readonly number[], intensity: number): [number, number, number] {
  return [color[0]*intensity, color[1]*intensity, color[2]*intensity];
}
