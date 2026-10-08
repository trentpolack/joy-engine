// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { VIEWPORT_GESTURE } from '../../../../site/src/interaction/constants.ts';
import { FORM_VIEW_MODE } from '../constants.ts';
import { GPU_BACKEND } from 'joy-engine/constants';
import type { RgbaColor } from 'joy-engine';
import type { GeometryData } from 'joy-engine/form';
import {ViewportInput} from '../../../../site/src/interaction/viewport-input.ts';
import {
  GpuTriangleRenderer,
  EditorGrid,
  OrbitCamera,
  sortCameraTransparentTriangles
} from 'joy-engine';
import { InspectionOverlay } from './inspection-overlay.ts';
import { pointBatch } from './batches.ts';
import wgsl from '../../shaders/preview.wgsl?raw';
import vertex from '../../shaders/preview.vert.glsl?raw';
import fragment from '../../shaders/preview.frag.glsl?raw';

// Neutral inspection light is editor-only and never enters geometry/material exports.
const EDITOR_LIGHTING = {lightDirection:[0.35,0.85,0.4], lightColor:[3,3,3], ambientColor:[0.45,0.45,0.45]};

const EDITOR_BACKGROUND: RgbaColor = [0.08,0.10,0.13,1];

/** Owns camera input, demand-driven rendering, and all GPU resources. */
export class Viewport {
  declare canvas: HTMLCanvasElement;
  declare onError: (message: string) => void;
  declare camera: OrbitCamera;
  declare renderer: GpuTriangleRenderer | null;
  declare data: GeometryData | null;
  declare grid: EditorGrid;
  declare gridHeight: number;
  declare mode: string;
  declare showGrid: boolean;
  declare pointSize: number;
  declare frameRequest: number;
  declare disposed: boolean;
  declare dataRevision: number;
  declare displayRevision: number;
  declare dragging: boolean;
  declare pointerStart: number[];
  declare inspection: InspectionOverlay;
  declare onPick: ((index: number) => void) | null;
  declare listeners: AbortController;
  declare observer: ResizeObserver;
  declare input: ViewportInput;

  /** @param canvas @param onError */
  constructor(canvas: HTMLCanvasElement, onError: (message: string) => void) {
    this.canvas = canvas;
    this.onError = onError;
    this.camera = new OrbitCamera();

    this.renderer = null;

    this.data = null;

    this.grid = new EditorGrid();
    this.gridHeight = 0;
    this.mode = FORM_VIEW_MODE.SHADED;
    this.showGrid = true;
    this.pointSize = 2;
    this.frameRequest = 0;
    this.disposed = false;
    this.dataRevision = 0;
    this.displayRevision = 0;
    this.dragging = false;
    this.pointerStart = [0, 0];
    this.inspection = new InspectionOverlay();

    this.onPick = null;
    this.listeners = new AbortController();
    this.observer = new ResizeObserver(() => this.invalidate());
    this.observer.observe(canvas);
    this.input = new ViewportInput(canvas, {
      drag: (mode, dx, dy) => this.move(mode, dx, dy),
      zoom: delta => this.zoom(delta),
      shortcut: key => {
        if(key === 'f') {
          this.frame();
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
          this.invalidate();
        }
      },
      click: event => {
        const bounds = canvas.getBoundingClientRect();
        this.onPick?.(this.inspection.pick(event.clientX - bounds.left, event.clientY - bounds.top,
          bounds.width, bounds.height, this.camera, this.renderer?.backend === GPU_BACKEND.WEBGPU));
      }
    });
  }
  /** Create the owned renderer; dispose a late result if the viewport was closed. */
  async initialize() {
    if(this.disposed) {
      return;
    }
    const renderer = await GpuTriangleRenderer.create(this.canvas, {
      shaderSources: { wgsl, glsl: { vertex, fragment } },
      uniformBufferSize: 80,
      pbr: true,
      postProcessing: {},
      device: {
        backend:
          new URLSearchParams(location.search).get('backend') === GPU_BACKEND.WEBGL
            ? GPU_BACKEND.WEBGL
            : GPU_BACKEND.BEST_AVAILABLE
      }
    });
    if(this.disposed) {
      renderer.destroy();
      return;
    }
    this.renderer = renderer;
    if(this.data) {
      await renderer.setMaterialMesh(this.data);
    }
    if(this.disposed) {
      return;
    }
    this.invalidate();
    return renderer.backend;
  }
  /** Borrow compiled geometry and commit its CPU preview only after a successful GPU upload.
   * @param data
   * @param shouldFrame Whether to frame the accepted geometry in world space.
   * @returns Whether the geometry became the displayed preview.
   */
  setData(data: GeometryData, shouldFrame: boolean): Promise<boolean> {
    if(this.disposed) {
      return Promise.resolve(false);
    }
    const revision = ++this.dataRevision;
    this.canvas.dataset.requestedRevision = String(revision);
    if(this.renderer) {
      return this.renderer.setMaterialMesh(data).then(ready => {
        // A successful upload can remain active while a newer request decodes
        // or fails. Keep the CPU point/grid data paired with that accepted mesh.
        if(ready && !this.disposed) {
          this.applyData(data, shouldFrame, revision);
          return true;
        }
        return false;
      }).catch(error => {
        if(!this.disposed && revision === this.dataRevision) {
          this.onError(`Material preview unavailable: ${error instanceof Error ? error.message : String(error)}`);
        }
        this.canvas.dataset.requestedRevision = String(this.displayRevision);
        return false;
      });
    }
    this.applyData(data, shouldFrame, revision);
    return Promise.resolve(true);
  }
  /** @private @param data @param shouldFrame @param revision */
  private applyData(data: GeometryData, shouldFrame: boolean, revision: number) {
    this.data = data;
    this.displayRevision = revision;
    if(shouldFrame) {
      this.camera.frame(data.positions);
    }
    let floor = 0;
    for(let i = 1; i < data.positions.length; i+= 3) {
      floor = Math.min(floor, data.positions[i]);
    }
    this.gridHeight = floor - this.camera.radius*0.015;
    this.invalidate();
  }
  /** @param locations @param colors */
  setInspection(locations: number[][], colors: number[][] | null) {
    this.inspection.set(locations, colors);
    this.selectElement(-1);
  }
  /** @param index */
  selectElement(index: number) {
    this.inspection.selected = index;
    this.canvas.dataset.selectedElement = String(index);
    this.invalidate();
  }
  frame() {
    if(this.data) {
      this.camera.frame(this.data.positions);
      if(this.canvas.clientWidth < this.canvas.clientHeight) {
        this.camera.distance*=
          this.canvas.clientHeight/Math.max(1, this.canvas.clientWidth);
      }
    }
    this.invalidate();
  }
  /** Coalesce editor changes into one demand-driven animation frame. */
  invalidate() {
    if(this.disposed || this.frameRequest) {
      return;
    }
    this.frameRequest = requestAnimationFrame(() => {
      this.frameRequest = 0;
      this.render();
    });
  }
  render() {
    if(!this.renderer || this.disposed) {
      return;
    }
    try {
      const width = this.canvas.clientWidth,
        height = this.canvas.clientHeight;
      if(!width || !height) {
        return;
      }
      this.renderer.resize(width, height, Math.min(2, devicePixelRatio));
      const points = this.data
        ? pointBatch(
            this.data,
            this.mode === FORM_VIEW_MODE.POINTS,
            this.camera,
            this.pointSize,
            height,
          )
        : [];
      const inspection = this.inspection.markers(this.camera, height);
      const gridFrame = this.showGrid ? this.grid.frame({
        target: this.camera.target, backward: this.camera.basis().backward, distance: this.camera.distance,
        viewportHeightPixels: height, height: this.gridHeight
      }) : null;
      const grid = gridFrame?.triangles ?? [];
      const gridSpacing = gridFrame ? String(gridFrame.spacing) : 'off';
      if(this.canvas.dataset.gridSpacing !== gridSpacing) {
        this.canvas.dataset.gridSpacing = gridSpacing;
      }
      const vertices = new Float32Array(points.length + grid.length + inspection.length);
      vertices.set(points);
      vertices.set(grid, points.length);
      vertices.set(inspection, points.length + grid.length);
      const uniforms = new Float32Array(20);
      uniforms.set(
        this.camera.matrix(width/height, this.renderer.backend === GPU_BACKEND.WEBGPU),
      );
      uniforms.set([1, 1, 1, 1], 16);

      const opaque: number[] = [];

      const transparent: number[] = [];
      // Alpha belongs to the authored color; opaque geometry still owns depth writes.
      for(let offset = 0; offset < vertices.length; offset+= 21) {
        const triangle = vertices.subarray(offset, offset + 21);
        const target =
          triangle[6] < 1 || triangle[13] < 1 || triangle[20] < 1
            ? transparent
            : opaque;
        target.push(...triangle);
      }
      const sorted = sortCameraTransparentTriangles(
        transparent,
        this.camera.basis().backward,
      );
      const basis = this.camera.basis();
      const materialFrame = this.mode === FORM_VIEW_MODE.POINTS || !this.data ? null : {
        viewProjection: uniforms.subarray(0, 16), eye: basis.eye, backward: basis.backward, ...EDITOR_LIGHTING
      };
      this.renderer.render(opaque, uniforms, EDITOR_BACKGROUND, sorted, 0, [], materialFrame);
      this.canvas.dataset.renderedRevision = String(this.displayRevision);
    } catch (error) {
      this.onError(
        `Preview unavailable: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
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
    this.invalidate();
  }

  /** @param delta */
  zoom(delta: number) {
    this.camera.distance = Math.max(this.camera.radius*0.05, Math.min(this.camera.radius*100,
      this.camera.distance*Math.exp(Math.max(-200, Math.min(200, delta))*0.001)));
    this.invalidate();
  }
  /** Restore orientation, then fit accepted geometry. */
  resetCamera() {
    this.camera = new OrbitCamera();
    this.camera.frame(this.data?.positions ?? []);
    this.frame();
  }
  /** Cancel pending rendering and release listeners, observation and GPU resources. */
  destroy() {
    if(this.disposed) {
      return;
    }
    this.disposed = true;
    cancelAnimationFrame(this.frameRequest);
    this.input.destroy();
    this.listeners.abort();
    this.observer.disconnect();
    this.renderer?.destroy();
  }
}
