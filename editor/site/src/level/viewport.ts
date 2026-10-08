// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { VIEWPORT_GESTURE } from '../interaction/constants.ts';
import { LEVEL_TOOL } from './constants.ts';
import { PRIMITIVE_SHAPE } from 'joy-engine/constants';
import type { EntityDefinition } from 'joy-engine';
import { AssetScene, SceneRenderer, OrbitCamera, EditorGrid } from 'joy-engine';
import {SceneEffectsControls} from 'joy-engine/development';
import {ViewportInput} from '../interaction/viewport-input.ts';
import { workspaceActive } from '../workspace/tool-bridge.ts';

/**
 * Owns editor GPU resources and pointer listeners; borrows authored snapshots and edit callbacks. */
export class LevelViewport {
  declare canvas: HTMLCanvasElement;
  declare actions: { select: (id: string | null) => void; move: (position: number[]) => void; status: (message: string) => void; readText: (path: string) => Promise<string>; };
  declare levelScene: AssetScene;
  declare previousTime: number;
  declare camera: OrbitCamera;
  declare renderer: SceneRenderer;
  declare listeners: AbortController;
  declare disposed: boolean;
  declare mode: string;
  declare snap: number;
  declare frame: number;
  declare entities: EntityDefinition[];
  declare selectedId: string | null;
  declare pointer: { x: number; y: number; lastX: number; lastY: number; button: number; moved: boolean; } | null;
  declare input: ViewportInput | undefined;
  declare grid: EditorGrid;
  declare showGrid: boolean;
  declare sceneEffectsControls: SceneEffectsControls;
  declare previousActive: boolean;

  /** @param canvas
   * @param actions
   */
  constructor(canvas: HTMLCanvasElement, actions: {
    select: (id: string | null) => void;
    move: (position: number[]) => void;
    status: (message: string) => void;
    readText: (path: string) => Promise<string>;
}) {
    // Borrowed DOM and callbacks; the editor owns their lifetime.
    this.canvas = canvas;
    this.actions = actions;

    // Owned camera, retained scene, renderer and input subscription.
    this.levelScene = new AssetScene({readText: actions.readText});
    this.previousTime = 0;
    this.camera = new OrbitCamera();
    this.camera.distance = 26;
    this.camera.pitch = 0.7;
    this.renderer = new SceneRenderer(canvas, {camera: this.camera, sceneEffects:{}});
    const controlsContainer = canvas.parentElement?.querySelector<HTMLElement>('.viewport-toolbar');
    if(!controlsContainer) {
      throw new Error('Level viewport requires its toolbar mount point.');
    }
    this.sceneEffectsControls = new SceneEffectsControls({renderer:this.renderer,
      container:controlsContainer, onError:message => actions.status(message)});
    this.previousActive = false;
    this.listeners = new AbortController();
    this.disposed = false;

    // Authoring interaction state. Grid snapping is measured in world meters.
    this.mode = LEVEL_TOOL.SELECT;
    this.snap = 1;
    this.frame = 0;

    this.entities = [];

    this.selectedId = null;

    this.pointer = null;
    this.grid = new EditorGrid();
    this.showGrid = true;
    this.bind();
  }

  /**
   * Start rendering after GPU initialization, unless teardown won the startup race. */
  async initialize() {
    try {
      await this.renderer.initialize();
    } catch(error) {
      this.sceneEffectsControls.reportError(error);
      throw error;
    }
    if(this.disposed) {
      return;
    }
    this.canvas.dataset.ready = 'true';
    this.sceneEffectsControls.ready();
    this.actions.status('Select an entity, or add a primitive. Edits save through the workspace.');
    this.render();
  }

  /**
   * Borrow a validated authored snapshot for drawing and picking; never mutate it.
   * @param entities
   * @param selectedId
   */
  async sync(entities: EntityDefinition[], selectedId: string | null) {
    const accepted = await this.levelScene.sync(entities, selectedId);
    if(!accepted || this.disposed) {
      return;
    }
    this.entities = this.levelScene.entities.map(entity => {
      const mesh = this.levelScene.meshes.get(entity.id);
      if(!mesh) {
        return entity;
      }
      return {...entity, components: {...entity.components, render: {...entity.components.render,
        boundsMin: [...mesh.bounds.min], boundsMax: [...mesh.bounds.max]}}};
    });
    this.selectedId = selectedId;
    this.canvas.dataset.entities = String(this.entities.length);
    this.renderer.resetHistory();
  }

  /**
   * Frame the selected entity, or the whole level when there is no selection. */
  focus() {
    const selection = this.entities.find(entity => entity.id === this.selectedId);
    const entities = selection ? [selection] : this.entities;
    const points = [];
    for(const entity of entities) {
      const size = dimensions(entity);
      for(const sign of [-1, 1]) {
        points.push(...entity.transform.position.map((value, axis) => value + size[axis]*sign/2));
      }
    }
    this.camera.frame(points);
    this.camera.distance = Math.max(6, this.camera.distance);
    this.renderer.resetHistory();
  }

  /**
   * Cancel the frame loop and release owned input, GPU and retained scene resources. */
  destroy() {
    if(this.disposed) {
      return;
    }
    this.disposed = true;
    cancelAnimationFrame(this.frame);
    this.listeners.abort();
    this.input?.destroy();
    this.sceneEffectsControls.destroy();
    this.renderer.destroy();
    this.levelScene.destroy();
  }

  /** Restore the editor camera without changing the document. */
  resetCamera() {
    this.camera.target = [0, 0, 0];
    this.camera.yaw = 0.75;
    this.camera.pitch = 0.7;
    this.camera.radius = 10;
    this.camera.distance = 26;
    this.renderer.resetHistory();
  }

  /**
   * Skip GPU submission for hidden workspace tabs while keeping the frame loop alive. @private */
  private render() {
    if(this.disposed) {
      return;
    }
    const now = performance.now();
    const seconds = this.previousTime ? Math.min(0.1, (now - this.previousTime)/1000) : 0;
    this.previousTime = now;
    const active = workspaceActive();
    if(active) {
      if(!this.previousActive) {
        this.renderer.resetHistory();
      }
      this.levelScene.step(seconds);
      if(this.levelScene.error) {
        this.actions.status(this.levelScene.error);
      }
      const basis = this.camera.basis();
      const grid = this.showGrid ? this.grid.frame({
        target: this.camera.target, backward: basis.backward, distance: this.camera.distance,
        viewportHeightPixels: Math.max(1, this.canvas.clientHeight)
      }) : null;
      const gridSpacing = grid ? String(grid.spacing) : 'off';
      if(this.canvas.dataset.gridSpacing !== gridSpacing) {
        this.canvas.dataset.gridSpacing = gridSpacing;
      }
      try {
        this.renderer.render(this.levelScene.scene, {
          ambient: 0.65,
          transparentTriangles: [...(grid?.triangles ?? []), ...this.levelScene.particles(basis)],
          clearColor: [0.045, 0.065, 0.10, 1],
          lights: [{x: 8, y: 18, z: 12, r: 1, g: 0.88, b: 0.73, range: 80}],
          timeSeconds:now/1000, deltaTimeSeconds:seconds
        });
      } catch(error) {
        this.sceneEffectsControls.reportError(error);
      }
    }
    this.previousActive = active;
    this.frame = requestAnimationFrame(() => this.render());
  }

  /** @private */
  private bind() {
    this.input = new ViewportInput(this.canvas, {
      drag: (mode, dx, dy) => {
        if(mode === VIEWPORT_GESTURE.PAN) {
          const basis = this.camera.basis();
          const speed = this.camera.distance*0.0016;
          this.camera.target = this.camera.target.map((value, axis) => value + (-basis.right[axis]*dx + basis.up[axis]*dy)*speed);
        } else {
          this.camera.yaw-= dx*0.008;
          this.camera.pitch = Math.max(0.08, Math.min(1.5, this.camera.pitch + dy*0.008));
        }
      },
      zoom: delta => { this.camera.distance = Math.max(1, Math.min(400, this.camera.distance*Math.exp(Math.max(-200, Math.min(200, delta))*0.001))); },
      shortcut: key => {
        if(key === 'f') {
          this.focus();
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
            this.camera.pitch = Math.max(0.08, this.camera.pitch - 0.1);
          }
        }
      },
      click: event => {
        if(this.mode === LEVEL_TOOL.MOVE && this.selectedId) {
          const point = this.renderer.pickGround(event.clientX, event.clientY);
          if(point) {
            this.actions.move(point.map(value => this.snap > 0 ? Math.round(value/this.snap)*this.snap : value));
          }
        } else {
          const ray = this.renderer.screenRay(event.clientX, event.clientY);
          this.actions.select(ray ? pickEntity(this.entities, ray) : null);
        }
      }
    });
  }
}

/**
 * Pick the nearest primitive using a ray transformed into each entity's local coordinates.
 * @param entities
 * @param ray */
export function pickEntity(entities: readonly EntityDefinition[], ray: {
    origin: number[];
    direction: number[];
}): string | null {
  let closest = Infinity;
  let selected = null;
  for(const entity of entities) {
    const render = entity.components.render;
    const shape = render?.asset ? PRIMITIVE_SHAPE.BOX : render?.shape ?? (entity.components.effect ? PRIMITIVE_SHAPE.SPHERE : undefined);
    if(shape !== PRIMITIVE_SHAPE.BOX && shape !== PRIMITIVE_SHAPE.SPHERE) {
      continue;
    }
    const size = dimensions(entity);
    const boundsMin = vector(render?.boundsMin, [0, 0, 0]);
    const boundsMax = vector(render?.boundsMax, [0, 0, 0]);
    const center = boundsMin.map((value, axis) => (value + boundsMax[axis])/2);
    const renderSize = vector(render?.size, [1, 1, 1]);
    if(size.some(value => value === 0)) {
      continue;
    }
    const origin = inverseRotate(
      ray.origin.map((value, axis) => value - entity.transform.position[axis]),
      entity.transform.rotation
    ).map((value, axis) => (value - center[axis]*entity.transform.scale[axis]*renderSize[axis])/size[axis]);
    const direction = inverseRotate(ray.direction, entity.transform.rotation).map((value, axis) => value/size[axis]);
    let distance = Infinity;
    if(shape === PRIMITIVE_SHAPE.SPHERE) {
      const a = direction.reduce((sum, value) => sum + value*value, 0);
      const b = origin.reduce((sum, value, axis) => sum + value*direction[axis], 0);
      const c = origin.reduce((sum, value) => sum + value*value, 0) - 0.25;
      const discriminant = b*b - a*c;
      if(discriminant >= 0) {
        distance = (-b - Math.sqrt(discriminant))/a;
        if(distance < 0) {
          distance = (-b + Math.sqrt(discriminant))/a;
        }
      }
    } else {
      let near = 0;
      let far = Infinity;
      for(let axis = 0; axis < 3; axis++) {
        if(Math.abs(direction[axis]) < 1e-9) {
          if(Math.abs(origin[axis]) > 0.5) {
            far = -1;
          }
          continue;
        }
        const first = (-0.5 - origin[axis])/direction[axis];
        const second = (0.5 - origin[axis])/direction[axis];
        near = Math.max(near, Math.min(first, second));
        far = Math.min(far, Math.max(first, second));
      }
      if(far >= near) {
        distance = near;
      }
    }
    if(distance >= 0 && distance < closest) {
      closest = distance;
      selected = entity.id;
    }
  }
  return selected;
}

/**
 * Return full primitive dimensions after entity scale, in world meters.
 * @param entity
 */
function dimensions(entity: EntityDefinition) {
  const value = entity.components.render?.size;
  const size = Array.isArray(value) && value.length === 3 && value.every(item => typeof item === 'number' && Number.isFinite(item))
    ?  ((value) as number[])
    : [1, 1, 1];
  const minimum = vector(entity.components.render?.boundsMin, [0, 0, 0]);
  const maximum = vector(entity.components.render?.boundsMax, [1, 1, 1]);
  return entity.transform.scale.map((value, axis) => value*size[axis]*Math.max(0.01, maximum[axis] - minimum[axis]));
}

/**
 * Reverse XYZ Euler rotation, preserving ray parameter distance. @param vector @param rotation */
function inverseRotate(vector: number[], rotation: number[]) {
  let [x, y, z] = vector;
  const [rx, ry, rz] = rotation;
  [x, y] = [x*Math.cos(rz) + y*Math.sin(rz), -x*Math.sin(rz) + y*Math.cos(rz)];
  [x, z] = [x*Math.cos(ry) - z*Math.sin(ry), x*Math.sin(ry) + z*Math.cos(ry)];
  [y, z] = [y*Math.cos(rx) + z*Math.sin(rx), -y*Math.sin(rx) + z*Math.cos(rx)];
  return [x, y, z];
}

/** @param value @param fallback */
function vector(value: unknown, fallback: number[]): number[] {
  return Array.isArray(value) && value.length === 3 && value.every(item => typeof item === 'number' && Number.isFinite(item)) ? value : fallback;
}
