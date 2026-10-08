// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { VIEWPORT_GESTURE } from '../../../site/src/interaction/constants.ts';
import { GPU_BACKEND, GPU_TEXTURE_FORMAT } from 'joy-engine/constants';
import { PARTICLE_PROFILE_MODE } from './constants.ts';
import type { RgbaColor, ParticleEffect } from 'joy-engine';
import {ViewportInput} from '../../../site/src/interaction/viewport-input.ts';
import {
  GpuTriangleRenderer,
  EditorGrid,
  createParticleEffectBatches,
  ParticleTextureSet,
  PARTICLE_TEXTURE_SHADERS,
  OrbitCamera,
  createParticleRenderMetrics,
  collectParticleLights,
  LIT_TRIANGLE_SHADERS,
  createPointLightUniforms,
  POINT_LIGHT_UNIFORM_BYTES,
} from 'joy-engine';
import wgsl from '../shaders/particle.wgsl?raw';
import vertex from '../shaders/particle.vert.glsl?raw';
import fragment from '../shaders/particle.frag.glsl?raw';

const PREVIEW_BACKGROUNDS: Record<string, RgbaColor> = {
  midnight: [0.028, 0.05, 0.087, 1],
  slate: [0.09, 0.13, 0.19, 1],
  gray: [0.25, 0.25, 0.25, 1],
  cream: [0.75, 0.66, 0.47, 1],
  black: [0, 0, 0, 1],
};

const PREVIEW_RECEIVER_HEIGHT = -120;

export interface PreviewStatistics {
  geometryMs: number;
  submissionMs: number;
  sortMs: number;
  sceneBatches: number;
  triangles: number;
  vertices: number;
  uploadBytes: number;
  lights: number;
  omittedLights: number;
  range: string;
  byEmitter: {
      id: string;
      triangles: number;
      vertices: number;
      geometryMs: number;
      lights: number;
  }[];
}

export type PreviewPerformanceMode = 'disabled' | 'overview' | 'detailed';

const POST_PROCESSING = {
  toneMapper:  (('agx') as const),
  exposure: 1.05,
  bloomStrength: 0.8,
  bloomRadius: 0.025,
  bloomThreshold: 0.8,
  bloomKnee: 0.4,
  saturation: 1.12,
  contrast: 1.06,
  vignetteStrength: 0.24,
  vignetteRadius: 0.78,
  filmGrain: 0.015,
};

/** Owns canvas input and all GPU resources used by the particle preview. */
export class ParticleViewport {
  declare canvas: HTMLCanvasElement;
  declare onError: (message: string) => void;
  declare renderer: GpuTriangleRenderer | null;
  declare textures: ParticleTextureSet | null;
  declare texturesReady: boolean;
  declare gridVisible: boolean;
  declare grid: EditorGrid;
  declare receiverVisible: boolean;
  declare background: string;
  declare renderMetrics: { emitters: { id: string; triangles: number; vertices: number; geometryMs: number; }[]; sortMs: number; } | null;
  declare stats: PreviewStatistics;
  declare performanceMode: PreviewPerformanceMode;
  declare effect: ParticleEffect | null;
  declare camera: OrbitCamera;
  declare dragging: boolean;
  declare disposed: boolean;
  declare listeners: AbortController;
  declare observer: ResizeObserver;
  declare input: ViewportInput;

  /** @param canvas @param onError */
  constructor(canvas: HTMLCanvasElement, onError: (message: string) => void) {
    this.canvas = canvas;
    this.onError = onError;

    this.renderer = null;

    this.textures = null;
    this.texturesReady = false;
    this.gridVisible = true;
    this.grid = new EditorGrid();
    this.receiverVisible = false;
    this.background = 'midnight';

    this.renderMetrics = null;

    this.performanceMode = 'overview';
    this.stats = { geometryMs: 0, submissionMs: 0, sortMs: 0, sceneBatches: 0, triangles: 0,
      vertices: 0, uploadBytes: 0, lights: 0, omittedLights: 0, range: 'Starting…', byEmitter: [] };

    this.effect = null;

    this.camera = new OrbitCamera();
    this.camera.distance = 650;
    this.camera.radius = 250;
    this.dragging = false;
    this.disposed = false;
    this.listeners = new AbortController();
    this.observer = new ResizeObserver(() => this.render());
    this.observer.observe(canvas);

    this.input = new ViewportInput(canvas, {
      drag: (mode, dx, dy) => this.move(mode, dx, dy),
      zoom: delta => this.zoom(delta),
      shortcut: key => {
        if(key === 'f') {
          this.frameEffect();
        }
        else if(key === '0') {
          this.resetCamera();
        }
        else {
          if(key === 'arrowleft') {
            this.camera.yaw-= 0.1;
          }
          if(key === 'arrowright') {
            this.camera.yaw+= 0.1;
          }
          if(key === 'arrowup') {
            this.camera.pitch = Math.min(1.5, this.camera.pitch + 0.1);
          }
          if(key === 'arrowdown') {
            this.camera.pitch = Math.max(-1.5, this.camera.pitch - 0.1);
          }
          this.render();
        }
      }
    });
  }

  /** Create owned GPU resources, releasing late startup results after disposal. */
  async initialize() {
    const backendQuery = new URLSearchParams(location.search).get('backend');
    const renderer = await GpuTriangleRenderer.create(this.canvas, {
      shaderSources: { wgsl, glsl: { vertex, fragment } },
      texturedShaderSources: PARTICLE_TEXTURE_SHADERS,
      opaqueShaderSources: LIT_TRIANGLE_SHADERS,
      uniformBufferSize: POINT_LIGHT_UNIFORM_BYTES,
      postProcessing: POST_PROCESSING,
      device: { backend: backendQuery === GPU_BACKEND.WEBGL ? GPU_BACKEND.WEBGL : GPU_BACKEND.BEST_AVAILABLE },
    });

    if(this.disposed) {
      renderer.destroy();
      return '';
    }

    this.texturesReady = false;
    this.renderer = renderer;
    this.textures = new ParticleTextureSet(renderer);
    if(this.effect) {
      const effect = this.effect;
      if(await this.prepareEffect(effect) && this.effect === effect && !this.disposed) {
        this.setEffect(effect);
      }
    } else {
      this.render();
    }
    return renderer.backend;
  }

  /** Prepare images before the application commits a new simulation.
   * A failed decode leaves the current preview and GPU images intact.
   * @param effect
   */
  async prepareEffect(effect: ParticleEffect) {
    return this.textures ? this.textures.prepare(effect.definition) : true;
  }

  /** Borrow a runtime owned by the application, after preparing its images.
   * @param effect
   */
  setEffect(effect: ParticleEffect | null) {
    this.effect = effect;
    this.renderMetrics = effect && this.performanceMode === PARTICLE_PROFILE_MODE.DETAILED ? createParticleRenderMetrics(effect.definition) : null;
    this.textures?.activate(effect?.definition ?? null);
    this.texturesReady = true;
    this.render();
  }

  /** Enable summary timing or opt into per-particle geometry profiling without changing the effect. */
  setPerformanceMode(mode: PreviewPerformanceMode) {
    if(this.performanceMode === mode) {
      return;
    }
    this.performanceMode = mode;
    this.renderMetrics = this.effect && mode === PARTICLE_PROFILE_MODE.DETAILED ? createParticleRenderMetrics(this.effect.definition) : null;
    this.stats.byEmitter = [];
    this.stats.sortMs = 0;
    this.render();
  }

  /** Draw the borrowed effect at its current simulation state without advancing it. */
  render() {
    if(this.disposed || !this.renderer) {
      return;
    }

    try {
      const width = this.canvas.clientWidth;
      const height = this.canvas.clientHeight;
      if(width === 0 || height === 0) {
        return;
      }

      this.renderer.resize(width, height, Math.min(2, devicePixelRatio));
      const monitoring = this.performanceMode !== PARTICLE_PROFILE_MODE.DISABLED;
      const geometryStart = monitoring ? performance.now() : 0;
      const basis = this.camera.basis();
      const illumination = this.effect ? collectParticleLights(this.effect) : { lights: [], omitted: 0 };
      const opaque = this.receiverVisible ? this.receiverGeometry() : [];
      const grid = this.gridVisible ? this.grid.frame({
        target: this.camera.target, backward: basis.backward, distance: this.camera.distance,
        viewportHeightPixels: height, height: PREVIEW_RECEIVER_HEIGHT + 2
      }) : null;
      const gridTriangles = grid?.triangles ?? [];
      const gridSpacing = grid ? String(grid.spacing) : 'off';
      if(this.canvas.dataset.gridSpacing !== gridSpacing) {
        this.canvas.dataset.gridSpacing = gridSpacing;
      }
      const runs = this.effect && this.texturesReady
        ? createParticleEffectBatches(this.effect, basis, this.renderMetrics ?? undefined, gridTriangles)
        : gridTriangles.length ? [{vertices: Array.from(gridTriangles), source: undefined}] : [];
      const batches = runs.map(batch => ({
        vertices: batch.vertices,
        ...(batch.source ? {texture: this.textures?.get(batch.source)} : {})
      }));
      const geometryMs = monitoring ? performance.now() - geometryStart : 0;
      const uniforms = createPointLightUniforms(
        this.camera.matrix(width / height, this.renderer.backend === GPU_BACKEND.WEBGPU), illumination.lights, 0.7,
      );
      const submissionStart = monitoring ? performance.now() : 0;
      this.renderer.render(opaque, uniforms, PREVIEW_BACKGROUNDS[this.background] ?? PREVIEW_BACKGROUNDS.midnight,
        [], this.effect?.time ?? 0, batches);
      if(!monitoring) {
        return;
      }
      const submissionMs = performance.now() - submissionStart;
      const vertices = opaque.length/7 + batches.reduce((count, batch) => count + batch.vertices.length / (batch.texture ? 9 : 7), 0);
      this.stats = {
        geometryMs, submissionMs, sortMs: this.renderMetrics?.sortMs ?? 0,
        // Nonempty geometry batches in this scene; post-processing draws are separate.
        sceneBatches: (opaque.length > 0 ? 1 : 0) + batches.filter(batch => batch.vertices.length > 0).length,
        triangles: vertices / 3, vertices,
        uploadBytes: (opaque.length + batches.reduce((count, batch) => count + batch.vertices.length, 0)) * 4,
        lights: illumination.lights.length, omittedLights: illumination.omitted,
        range: `${this.renderer.postprocessor?.format === GPU_TEXTURE_FORMAT.RGBA16FLOAT ? 'HDR' : 'SDR'} Scene → SDR Output`,
        byEmitter: this.renderMetrics?.emitters.map((row, index) => ({
          ...row, lights: illumination.lights.filter(light => light.emitterIndex === index).length,
        })) ?? [],
      };
    } catch (error) {
      this.onError(error instanceof Error ? error.message : String(error));
    }
  }

  /** Preview settings are presentation only and never alter the exported effect.
   * @param options
   */
  setPreviewOptions(options: {
    gridVisible?: boolean;
    receiverVisible?: boolean;
    background?: string;
}) {
    if(options.background !== undefined) {
      if(!Object.hasOwn(PREVIEW_BACKGROUNDS, options.background)) {
        throw new RangeError('Unknown preview background.');
      }
      this.background = options.background;
    }
    if(options.gridVisible !== undefined) {
      this.gridVisible = options.gridVisible;
    }
    if(options.receiverVisible !== undefined) {
      this.receiverVisible = options.receiverVisible;
    }
    this.render();
  }

  /** @param mode @param dx @param dy */
  move(mode: typeof VIEWPORT_GESTURE.PAN | typeof VIEWPORT_GESTURE.ORBIT, dx: number, dy: number) {
    if(mode === VIEWPORT_GESTURE.PAN) {
      const {right, up} = this.camera.basis();
      const scale = this.camera.distance/Math.max(100, this.canvas.clientHeight);
      this.camera.target = this.camera.target.map((value, axis) => value - dx*scale*right[axis] + dy*scale*up[axis]);
    } else {
      this.camera.yaw-= dx*0.007;
      this.camera.pitch = Math.max(-1.5, Math.min(1.5, this.camera.pitch + dy*0.007));
    }
    this.render();
  }

  /** @param deltaY */
  zoom(deltaY: number) {
    this.camera.distance = Math.max(
      5,
      Math.min(
        100000,
        this.camera.distance * Math.exp(Math.max(-200, Math.min(200, deltaY)) * 0.001),
      ),
    );
    this.render();
  }

  resetCamera() {
    this.camera = new OrbitCamera();
    this.camera.distance = 650;
    this.camera.radius = 250;
    this.render();
  }

  frameEffect() {
    const positions = [];
    for(const particle of this.effect?.particles ?? []) {
      positions.push(
        particle.x - particle.size,
        particle.y - particle.size,
        particle.z - particle.size,
        particle.x + particle.size,
        particle.y + particle.size,
        particle.z + particle.size,
      );
    }
    if(!positions.length) {
      this.resetCamera();
      return;
    }
    this.camera.frame(positions);
    this.camera.distance *= Math.max(
      1,
      this.canvas.clientHeight / Math.max(1, this.canvas.clientWidth),
    );
    this.render();
  }

  /** A matte ground surface shows attached lights independently of grid lines. */
  receiverGeometry() {

    const vertices: number[] = [];
    for(const [x, z] of [[-500,-500],[500,-500],[500,500],[-500,-500],[500,500],[-500,500]]) {
      vertices.push(x, PREVIEW_RECEIVER_HEIGHT, z, 0.075, 0.085, 0.105, 1);
    }
    return vertices;
  }

  /** Release input, resize observation, textures and renderer; the application owns the effect. */
  destroy() {
    if(this.disposed) {
      return;
    }

    this.disposed = true;
    this.input.destroy();
    this.listeners.abort();
    this.observer.disconnect();
    this.textures?.destroy();
    this.textures = null;
    this.renderer?.destroy();
    this.renderer = null;
  }
}
