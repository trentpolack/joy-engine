// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { ShadowOptions } from '../shadows/types.ts';
import type {
  RendererDiagnosticOptions,
  RendererDiagnostics
} from '../gpu/gpu-diagnostics.ts';
import type { PostProcessingConfig } from '../postprocessor/config.ts';
import type { GpuDeviceOptions } from '../gpu/gpu-device.ts';
import type { EnvironmentFrame } from '../../environment/environment-system.ts';
import type { PointLight } from '../lighting/point-lights.ts';
import type {
  Color,
  Vector2
} from '../../core/types.ts';
import type { RenderScene } from './render-scene.ts';
import type {
  SceneEffectsOptions,
  SceneEffectsFrame
} from './scene-effects.ts';
import type { SceneTemporalSnapshot } from './scene-temporal.ts';
import type { SceneAttachmentFrame } from './scene-effect-attachments.ts';
import type { FrameView } from './frame-view.ts';
import type { WorldRay } from '../camera/projection.ts';
import type {NativeSceneGeometry} from './native-geometry.ts';
import type {TerrainInstance} from './terrain-instance.ts';
import type {PreparedTerrainPacket} from './scene-geometry.ts';

import { GPU_BACKEND } from '../gpu/constants.ts';
import { normalizeSceneEffectsOptions } from './scene-effect-config.ts';
import { prepareSceneTemporalFrame } from './scene-temporal.ts';
import { sceneCameraJitter } from './scene-camera-frame.ts';
import { GpuTriangleRenderer } from '../renderer.ts';
import { OrbitCamera } from '../camera/orbit-camera.ts';
import {
  projectToViewport,
  screenRay,
  intersectRayPlane
} from '../camera/projection.ts';
import {
  LIT_TRIANGLE_SHADERS,
  POINT_LIGHT_UNIFORM_BYTES,
  createPointLightUniforms
} from '../lighting/point-lights.ts';
import { createAtmosphereRenderUniforms } from '../environment/atmosphere-rendering.ts';
import { gpuBackendFromQuery } from '../gpu/gpu-device.ts';
import {TerrainScenePass} from '../terrain/terrain-scene-pass.ts';
import { prepareSceneFrame } from './prepare-frame.ts';
import wgsl from '../../../shaders/rendering/scene/scene.wgsl?raw';
import vertex from '../../../shaders/rendering/scene/scene.vert.glsl?raw';
import fragment from '../../../shaders/rendering/scene/scene.frag.glsl?raw';

/**
 * Construction settings; the renderer borrows its canvas and camera and owns GPU resources.
 */
export interface SceneRendererOptions {
  /**
   * WebGPU geometric shadows default to enabled. WebGL 2 retains illumination without shadows.
   */
  shadows?: ShadowOptions | false;
  camera?: OrbitCamera;
  atmosphere?: boolean;
  pixelRatio?: number;
  postProcessing?: Partial<PostProcessingConfig> | false;
  device?: GpuDeviceOptions;
  /**
   * Individual WebGPU effect opt-ins. Opaque scene effects precede transparency and HDR display processing.
   */
  sceneEffects?: SceneEffectsOptions;
}

/**
 * Borrowed scene inputs for one submission; time is in seconds and geometry is world-space.
 */
export interface SceneFrameOptions {
  environment?: EnvironmentFrame;
  lights?: readonly PointLight[];
  ambient?: number;
  clearColor?: Color;
  timeSeconds?: number;
  opaqueTriangles?: readonly number[];
  transparentTriangles?: readonly number[];
  culling?: boolean;
  /**
   * Seconds since the preceding rendered frame, for exposure adaptation.
   */
  deltaTimeSeconds?: number;
  /**
   * Explicit camera cut, teleport, procedural topology change or other discontinuity.
   */
  resetHistory?: boolean;
}

/**
 * Successful camera submission retained for cut detection and temporal reprojection.
 */
interface PreviousSceneCamera {
  camera: OrbitCamera;
  eye: readonly number[];
  yaw: number;
  pitch: number;
  projection: readonly number[];
  matrix: readonly number[];
  rasterMatrix: readonly number[];
  jitter: readonly [number, number];
  time: number;
}

type SceneQueues = ReturnType<typeof prepareSceneFrame>;
type SceneTemporalFrame = ReturnType<typeof prepareSceneTemporalFrame>;

/**
 * CPU effect inputs prepared together; temporal state remains pending until submission succeeds.
 */
interface PreparedSceneEffects {
  frame: SceneEffectsFrame;
  attachments: SceneAttachmentFrame | null;
  temporal: SceneTemporalFrame | null;
}

const PERSPECTIVE_PROJECTION = 'perspective';
const FLOAT32_FILTERABLE_FEATURE = 'float32-filterable';

/**
 * Owns GPU frame preparation and disposal; borrows camera, scenes and transient geometry.
 * Standard mode is flat-lit vertex color for opaque meshes and unlit transparency.
 * Authored PBR channels remain on assets; this adapter does not interpret their materials.
 */
export class SceneRenderer {
  // Borrowed host and mutable camera; the options record is owned by this instance.
  readonly canvas: HTMLCanvasElement;
  camera: OrbitCamera;
  readonly options: SceneRendererOptions;

  // Owned GPU renderer and its pending initialization.
  private renderer: GpuTriangleRenderer | null = null;
  private initialization: Promise<void> | null = null;
  destroyed = false;

  // CSS viewport dimensions and backing-pixel density.
  width = 0;
  height = 0;
  pixelRatio = 0;
  stats = {visible: 0, culled: 0};

  // History is committed only after a successful submission.
  private temporalSnapshot: SceneTemporalSnapshot | null = null;
  private previousCamera: PreviousSceneCamera | null = null;
  private frameIndex = 0;
  private readonly terrainAdapters = new Map<TerrainInstance, TerrainScenePass>();

  /**
   * Configure a scene renderer without allocating GPU resources.
   * @param canvas Borrowed canvas; its CSS dimensions determine the visible view.
   * @param options Borrowed settings copied into this owner; camera remains borrowed.
   * @throws If requested effects are invalid or the offscreen display path is disabled.
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
    this.options = {
      ...options,
      sceneEffects: options.sceneEffects ? structuredClone(options.sceneEffects) : undefined
    };
  }

  /**
   * Initialize once; destruction during startup releases the late GPU result.
   * @returns Completion of the shared initialization attempt.
   * @throws If this owner was destroyed or GPU initialization fails.
   */
  async initialize() {
    if(this.destroyed) {
      throw new Error('SceneRenderer is destroyed.');
    }
    this.initialization ??= this.initializeRenderer();
    await this.initialization;
  }

  /**
   * @returns Whether an initialized GPU renderer is available and this owner is alive.
   */
  get ready() {
    return((this.renderer) !== null && !this.destroyed);
  }
  /**
   * @returns The active backend, or null before initialization and after disposal.
   */
  get backend() {
    return(this.renderer?.backend ?? null);
  }
  /**
   * @returns The offscreen color format, or null when no offscreen target is available.
   */
  get colorFormat() {
    return(this.renderer?.postprocessor?.format ?? null);
  }

  /**
   * Read-only display settings snapshot for opt-in controls; null on a direct canvas path.
   */
  get postProcessingConfig(): Readonly<PostProcessingConfig> | null {
    return this.renderer?.postProcessing ? Object.freeze({...this.renderer.postProcessing}) : null;
  }

  /**
   * Backend requirements and requested effect availability; null before initialization.
   */
  get sceneEffectsCapabilities() {
    return this.renderer?.sceneEffects?.capabilities ?? null;
  }

  /**
   * @returns Borrowed clustered-lighting statistics, or null while that pass is inactive.
   */
  get sceneVolumeStats() {
    return this.renderer?.sceneVolumeStats ?? null;
  }

  /**
   * Replace individual opt-ins atomically; preserves the working graph if validation fails.
   * @param config Borrowed effect settings copied after successful graph replacement.
   * @throws If initialization is incomplete or effect validation/allocation fails.
   */
  setSceneEffectsConfig(config: SceneEffectsOptions) {
    if(!this.ready || !this.renderer) {
      throw new Error('Initialize SceneRenderer before updating scene effects.');
    }
    normalizeSceneEffectsOptions(config);
    this.renderer.setSceneEffectsConfig(config);
    this.options.sceneEffects = structuredClone(config);
    this.resetHistory();
  }

  /**
   * Call after teleports, discontinuous animation or content edits that retain instance identities.
   */
  resetHistory() {
    this.temporalSnapshot = null;
    this.previousCamera = null;
    this.frameIndex = 0;
    this.renderer?.resetSceneHistory();
  }

  /**
   * Copy requested global luma counters and this scene's last culling counts.
   * No sampling runs unless called; returns null before initialization or after disposal.
   * @param options Selects optional global luma counters for this sample.
   * @returns An owned diagnostic snapshot, or null when the GPU renderer is unavailable.
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
   * @param scene Borrowed scene; preparing its queues may refresh instance geometry caches.
   * @param frame Borrowed per-frame geometry, lighting, environment, and timing inputs.
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
    const time = frame.timeSeconds ?? 0;
    const view = this.prepareFrameView(this.renderer, scene, frame, effectsEnabled, webgpu, time);
    const queues = prepareSceneFrame(scene, {
      backward: view.camera.backward,
      viewProjection: frame.culling === false ? undefined : view.camera.viewProjectionMatrix,
      webgpu,
      opaqueTriangles: frame.opaqueTriangles,
      transparentTriangles: frame.transparentTriangles
    });
    const nativeGeometry = this.prepareTerrainGeometry(
      queues.terrainPackets,
      view.camera.rasterViewProjectionMatrix,
      view.camera.eye,
      this.height*Math.abs(view.camera.projectionMatrix[5])/2
    );
    const uniforms = createPointLightUniforms(
      view.camera.rasterViewProjectionMatrix,
      view.lighting.lights,
      view.lighting.ambient,
      view.environment
    );
    const atmosphereUniforms = this.prepareAtmosphere(view);
    const effects = this.prepareEffects(scene, frame, view, queues, nativeGeometry, effectsEnabled, time);

    try {
      this.renderer.render(
        queues.opaque,
        uniforms,
        frame.clearColor ?? [0, 0, 0, 1],
        queues.transparent,
        time,
        [],
        null,
        effects.frame,
        atmosphereUniforms,
        effects.attachments,
        {view, casters: queues.shadowCasters, receivesShadowRanges: queues.receivesShadowRanges},
        nativeGeometry
      );
    } catch(error) {
      if(effectsEnabled) {
        this.resetHistory();
      }
      throw error;
    }
    if(effects.temporal) {
      this.commitHistory(effects.temporal, view, time);
    }

    this.stats = {visible: queues.visible, culled: queues.culled};
    return true;
  }

  /**
   * Client CSS pixels to a world ray; independent of backing pixel density.
   * @param clientX Horizontal client position in CSS pixels.
   * @param clientY Vertical client position in CSS pixels.
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
   * @param x Horizontal client position in CSS pixels.
   * @param y Vertical client position in CSS pixels.
   * @param height Horizontal plane height in world units.
   * @returns World position or null if the canvas is hidden, has no area, or the ray is parallel to the plane.
   */
  pickGround(x: number, y: number, height: number = 0): readonly number[] | null {
    return(intersectRayPlane(this.screenRay(x, y), [0, 1, 0], -height));
  }

  /**
   * World position to canvas-local CSS pixels, or null behind camera/when hidden.
   * @param position Borrowed XYZ position in world units; never modified.
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
   * @param config Borrowed replacement display settings; omitted fields reset to defaults.
   * @throws If initialization is incomplete or settings are invalid.
   */
  setPostProcessingConfig(config: Partial<PostProcessingConfig>) {
    if(!this.renderer) {
      throw new Error('Initialize SceneRenderer before updating postprocessing.');
    }

    this.renderer.setPostProcessingConfig(config);
  }

  /**
   * Idempotent. Does not destroy borrowed scene instances or camera state.
   */
  destroy() {
    if(this.destroyed) {
      return;
    }

    this.resetHistory();
    this.destroyed = true;
    for(const adapter of this.terrainAdapters.values()) {
      adapter.destroy();
    }
    this.terrainAdapters.clear();
    this.renderer?.destroy();
    this.renderer = null;
  }

  /** Prepare resident terrain adapters before any render pass opens. */
  private prepareTerrainGeometry(
    packets: readonly PreparedTerrainPacket[],
    matrix: ArrayLike<number>,
    eye: readonly number[],
    projectionScale: number): NativeSceneGeometry[] {
    const renderer = this.renderer!;
    const active = new Set(packets.map(packet => packet.instance));
    for(const [instance, adapter] of this.terrainAdapters) {
      if(!active.has(instance)) {
        adapter.destroy();
        this.terrainAdapters.delete(instance);
      }
    }

    const geometry: NativeSceneGeometry[] = [];
    for(const packet of packets) {
      if(!packet.visible && !packet.castsShadow) {
        continue;
      }
      const instance = packet.instance;
      const previous = this.terrainAdapters.get(instance);
      const adapter = previous ?? new TerrainScenePass(
        renderer.device,
        instance.asset,
        renderer.postprocessor?.format,
        this.options.shadows
      );
      if(!previous) {
        this.terrainAdapters.set(instance, adapter);
      }
      try {
        adapter.prepare(matrix, packet.worldTransform, {
          eye: Array.from(eye),
          target: Array.from(this.camera.target),
          projectionScale,
          tolerance: instance.tolerance,
          targetRadius: instance.targetRadius
        }, instance.diagnostic);
      } catch(error) {
        if(!previous) {
          adapter.destroy();
          this.terrainAdapters.delete(instance);
        }
        throw error;
      }
      geometry.push({
        visible: packet.visible,
        castsShadow: packet.castsShadow,
        receivesShadow: packet.receivesShadow,
        draw: (pass, bindings, shadowed, additive) => adapter.draw(pass, bindings, shadowed, additive),
        drawShadow: (pass, lightMatrix) => adapter.drawShadow(pass, lightMatrix),
        drawSurface: (pass, viewProjection, viewMatrix) => adapter.drawSurface(pass, viewProjection, viewMatrix),
        drawMotion: (pass, motionFrame) => adapter.drawMotion(pass, motionFrame)
      });
    }
    return geometry;
  }

  /**
   * Detect camera/time discontinuities before computing raster jitter and the immutable view.
   * @param renderer Borrowed initialized GPU renderer, validated by the public render path.
   * @param scene Borrowed scene identity used to reject unrelated temporal history.
   * @param frame Borrowed frame settings, including explicit reset requests.
   * @param effectsEnabled Whether this submission uses the temporal effect path.
   * @param webgpu Whether camera projection uses WebGPU clip depth.
   * @param time Current scene time in seconds.
   * @returns An owned immutable view using this renderer's shadow configuration.
   */
  private prepareFrameView(
    renderer: GpuTriangleRenderer,
    scene: RenderScene,
    frame: SceneFrameOptions,
    effectsEnabled: boolean,
    webgpu: boolean,
    time: number): FrameView {
    const viewMatrix = this.camera.viewMatrix();
    const projection = this.camera.projectionMatrix(this.width/this.height, webgpu);
    const backingWidth = Math.round(this.width*this.pixelRatio);
    const backingHeight = Math.round(this.height*this.pixelRatio);
    const basis = this.camera.basis();
    const prior = this.previousCamera;
    const timeDiscontinuity = prior && ((time < prior.time) || ((time - prior.time) > 0.25));
    const cameraCut = prior && (
      prior.camera !== this.camera ||
      Math.hypot(...basis.eye.map((value, axis) => value - prior.eye[axis])) > this.camera.distance*0.5 ||
      Math.abs(this.camera.yaw - prior.yaw) > 0.5 ||
      Math.abs(this.camera.pitch - prior.pitch) > 0.5 ||
      projection.some((value, index) => value !== prior.projection[index])
    );
    const sceneChanged = this.temporalSnapshot?.scene !== scene;
    if(effectsEnabled && (frame.resetHistory || timeDiscontinuity || cameraCut || sceneChanged)) {
      this.resetHistory();
    }

    const taa = this.options.sceneEffects?.taa;
    const jitter: [number, number] = effectsEnabled && taa
      ? sceneCameraJitter(this.frameIndex, backingWidth, backingHeight)
      : [0, 0];

    return renderer.createFrameView({
      camera: {
        viewMatrix,
        projectionMatrix: projection,
        nearPlane: this.camera.nearPlane,
        farPlane: this.camera.farPlane,
        eye: basis.eye,
        backward: basis.backward
      },
      viewport: {
        cssWidth: this.width,
        cssHeight: this.height,
        pixelRatio: this.pixelRatio
      },
      jitter,
      lights: frame.lights,
      ambient: frame.ambient,
      environment: frame.environment
    });
  }

  /**
   * Pack sky inputs from the same raster camera used by opaque scene geometry.
   * @param view Borrowed immutable view; never modified.
   * @returns Owned packed uniforms, or null when no environment was submitted.
   * @throws If atmosphere was not enabled or the raster camera cannot be inverted.
   */
  private prepareAtmosphere(view: FrameView): Float32Array<ArrayBuffer> | null {
    if(!view.environment) {
      return null;
    }
    if(!this.options.atmosphere) {
      throw new Error('Environment frames require atmosphere at SceneRenderer creation.');
    }
    if(!view.camera.inverseRasterViewProjectionMatrix) {
      throw new Error('Atmosphere requires an invertible camera matrix.');
    }

    return createAtmosphereRenderUniforms(
      view.camera.inverseRasterViewProjectionMatrix,
      view.camera.eye,
      [view.viewport.width, view.viewport.height],
      view.environment
    );
  }

  /**
   * Prepare motion correspondence and effect attachments without committing temporal history.
   * @param scene Borrowed scene whose mesh identities define correspondence.
   * @param frame Borrowed time and effect settings for this submission.
   * @param view Borrowed immutable camera, light, and viewport snapshot.
   * @param queues Borrowed prepared geometry and surface metadata.
   * @param enabled Whether WebGPU scene effects are active.
   * @param time Current scene time in seconds.
   * @returns Owned CPU records borrowing view/queues, plus an uncommitted temporal snapshot.
   * @throws If temporal geometry is invalid or required camera transforms are singular.
   */
  private prepareEffects(
    scene: RenderScene,
    frame: SceneFrameOptions,
    view: FrameView,
    queues: SceneQueues,
    nativeGeometry: readonly NativeSceneGeometry[],
    enabled: boolean,
    time: number): PreparedSceneEffects {
    if(!enabled) {
      return {frame: {}, attachments: null, temporal: null};
    }

    const camera = view.camera;
    const temporal = prepareSceneTemporalFrame(this.temporalSnapshot, {
      scene,
      vertices: queues.opaque,
      ranges: queues.opaqueRanges,
      viewProjection: camera.viewProjectionMatrix
    });
    const previous = this.previousCamera;
    const previousJitter = previous?.jitter ?? camera.jitter;
    const inverseProjection = camera.inverseProjectionMatrix;
    const inverseView = camera.inverseViewMatrix;
    const inverseViewProjection = camera.inverseViewProjectionMatrix;
    const inverseRasterViewProjection = camera.inverseRasterViewProjectionMatrix;
    if(!inverseProjection || !inverseView || !inverseViewProjection || !inverseRasterViewProjection) {
      throw new Error('Scene effects require invertible perspective camera matrices.');
    }

    const deltaTimeSeconds = frame.deltaTimeSeconds ?? (previous ? Math.max(0, time - previous.time) : 0);
    const effectsFrame: SceneEffectsFrame = {
      nearPlane: camera.nearPlane,
      farPlane: camera.farPlane,
      projection: PERSPECTIVE_PROJECTION,
      projectionMatrix: camera.rasterProjectionMatrix,
      inverseProjectionMatrix: inverseProjection,
      inverseViewMatrix: inverseView,
      inverseViewProjectionMatrix: inverseViewProjection,
      previousViewProjectionMatrix: previous?.matrix ?? camera.viewProjectionMatrix,
      inverseRasterViewProjectionMatrix: inverseRasterViewProjection,
      previousRasterViewProjectionMatrix: previous?.rasterMatrix ?? camera.rasterViewProjectionMatrix,
      currentJitter: camera.jitter,
      previousJitter,
      resetHistory: temporal.resetHistory,
      frameIndex: this.frameIndex,
      timeSeconds: time,
      deltaTimeSeconds,
      directionalLightDirectionView: view.environment
        ? transformDirection(camera.viewMatrix, view.environment.primaryLight.direction)
        : [0, 1, 0],
      directionalLightColor: view.environment
        ? scaleLightColor(view.environment.primaryLight.color, view.environment.primaryLight.intensity)
        : [0, 0, 0]
    };
    const attachments: SceneAttachmentFrame = {
      motion: {
        vertices: queues.opaque,
        previousPositions: temporal.previousPositions,
        currentViewProjection: camera.viewProjectionMatrix,
        previousViewProjection: temporal.previousViewProjection,
        rasterViewProjection: camera.rasterViewProjectionMatrix,
        velocityJitterDelta: [camera.jitter[0] - previousJitter[0], camera.jitter[1] - previousJitter[1]],
        width: view.viewport.width,
        height: view.viewport.height
      },
      view,
      surfaceRoughness: queues.surfaceRoughness,
      nativeGeometry
    };

    return {frame: effectsFrame, attachments, temporal};
  }

  /**
   * Commit correspondence only after the GPU renderer accepts the complete submission.
   * @param temporal Owned pending geometry history transferred into this owner.
   * @param view Immutable camera arrays retained for the next rendered frame.
   * @param time Successful scene submission time in seconds.
   */
  private commitHistory(temporal: SceneTemporalFrame, view: FrameView, time: number): void {
    this.temporalSnapshot = temporal.snapshot;
    this.previousCamera = {
      camera: this.camera,
      eye: view.camera.eye,
      yaw: this.camera.yaw,
      pitch: this.camera.pitch,
      projection: view.camera.projectionMatrix,
      matrix: view.camera.viewProjectionMatrix,
      rasterMatrix: view.camera.rasterViewProjectionMatrix,
      jitter: view.camera.jitter,
      time
    };
    this.frameIndex+= 1;
  }

  /**
   * Initialize the GPU renderer.
   * @returns Completion after taking ownership of the renderer or releasing a late result.
   * @throws If device, pipeline, or optional feature initialization fails.
   */
  private async initializeRenderer() {
    const renderer = await GpuTriangleRenderer.create(this.canvas, {
      shaderSources: {wgsl, glsl: {vertex, fragment}},
      opaqueShaderSources: LIT_TRIANGLE_SHADERS,
      uniformBufferSize: POINT_LIGHT_UNIFORM_BYTES,
      atmosphere: this.options.atmosphere,
      shadows: this.options.shadows,
      sceneEffects: this.options.sceneEffects,
      postProcessing: this.options.postProcessing === false ? undefined : (this.options.postProcessing ?? {}),
      device: {
        ...(this.options.device ?? {backend: gpuBackendFromQuery(globalThis.location?.search ?? '')}),
        optionalFeatures: [...(this.options.device?.optionalFeatures ?? []), FLOAT32_FILTERABLE_FEATURE]
      }
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
    }
    if(!Number.isFinite(pixelRatio) || (pixelRatio <= 0)) {
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

/**
 * Rotate a borrowed world-space light direction into the camera's view basis.
 * @param view Borrowed column-major view matrix; never modified.
 * @param direction Borrowed world-space XYZ light direction.
 * @returns An owned direction vector in view space.
 */
function transformDirection(view: ArrayLike<number>, direction: readonly number[]): [number, number, number] {
  return [
    view[0]*direction[0] + view[4]*direction[1] + view[8]*direction[2],
    view[1]*direction[0] + view[5]*direction[1] + view[9]*direction[2],
    view[2]*direction[0] + view[6]*direction[1] + view[10]*direction[2]
  ];
}

/**
 * Scale linear RGB radiance without modifying the borrowed color.
 * @param color Borrowed linear RGB light color.
 * @param intensity Scalar radiance multiplier.
 * @returns An owned linear RGB vector.
 */
function scaleLightColor(color: readonly number[], intensity: number): [number, number, number] {
  return [color[0]*intensity, color[1]*intensity, color[2]*intensity];
}
