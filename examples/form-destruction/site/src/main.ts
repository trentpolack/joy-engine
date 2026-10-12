// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {
  ChunkStructure, MeshAsset, OrbitCamera, RenderScene, SceneRenderer,
  gpuBackendFromQuery
} from 'joy-engine';
import type {MeshInstance, Vector3} from 'joy-engine';
import {DESTRUCTION_CONFIG, INTEGRITY_CONFIG, PRESENTATION_CONFIG, STRUCTURE_VIEW, SUPPORT_CONFIG, VIEW_CONFIG} from '../../src/config/destruction-config.ts';
import type {StructureView} from '../../src/config/destruction-config.ts';
import {generateStructure} from '../../src/structure.ts';
import {DestructionDebris} from '../../src/destruction-debris.ts';
import {ImpactDebugDraw} from '../../src/impact-debug.ts';
import {createStructureMeshData} from '../../src/structure-view.ts';

const EVENT = {
  click: 'click', input: 'input', keydown: 'keydown', pointerdown: 'pointerdown',
  pointermove: 'pointermove', pointerup: 'pointerup', pointercancel: 'pointercancel',
  lostpointercapture: 'lostpointercapture', wheel: 'wheel', pagehide: 'pagehide', blur: 'blur'
} as const;
const KEY = {reset: 'KeyR', escape: 'Escape'} as const;
type PointerGesture = {id: number; startX: number; startY: number; x: number; y: number; dragged: boolean};
type Impact = {chunkId: string; point: Vector3};
type DemoSnapshot = ReturnType<FormDestructionDemo['snapshot']>;
declare global {
  interface Window {
    formDestruction?: {
      reset(): void;
      snapshot(): DemoSnapshot;
      sample(frames: number): Promise<DemoSnapshot>;
    };
  }
}

/**
 * Owns one destructible structure, its scene, input listeners, and animation loop.
 * Damage and visible geometry change together at the next simulation boundary.
 */
class FormDestructionDemo {
  // Owned scene resources; the scene retains meshes until removal or disposal.
  private canvas = element<HTMLCanvasElement>('world');
  private camera = new OrbitCamera();
  private renderer = new SceneRenderer(this.canvas, {
    camera: this.camera,
    pixelRatio: Math.min(devicePixelRatio, VIEW_CONFIG.maximumPixelRatio),
    device: {backend: gpuBackendFromQuery(location.search), fallbackToWebGl: true}
  });
  private scene = new RenderScene();
  private structure = new ChunkStructure(generateStructure(), SUPPORT_CONFIG);
  private structureMesh: MeshInstance | null = null;
  private debris = new DestructionDebris(this.scene);
  private impactDebug = new ImpactDebugDraw(this.scene);

  // Input and local tuning belong to this example instance.
  private subscriptions = new AbortController();
  private pointer: PointerGesture | null = null;
  private damageRadius: number = DESTRUCTION_CONFIG.damageRadius;
  private damagePerHit: number = DESTRUCTION_CONFIG.damagePerHit;
  private lastImpact: Impact | null = null;
  private damagePending = false;
  private debugView: StructureView = INTEGRITY_CONFIG.enabled ? STRUCTURE_VIEW.integrity : STRUCTURE_VIEW.material;
  private detachedChunks = new Set<string>();

  // Bounded diagnostic samples measure CPU work, never GPU execution time.
  private animationId = 0;
  private previousTimestamp = 0;
  private frameCount = 0;
  private renderSamples: number[] = [];
  private damageSamples: number[] = [];
  private querySamples: number[] = [];
  private disposed = false;

  /**
   * Initialize rendering before binding input or starting the frame loop.
   */
  async initialize() {
    this.camera.target = [...VIEW_CONFIG.target];
    this.camera.distance = VIEW_CONFIG.distance;
    this.camera.radius = VIEW_CONFIG.radius;
    this.camera.yaw = VIEW_CONFIG.yaw;
    this.camera.pitch = VIEW_CONFIG.pitch;
    this.camera.nearClip = VIEW_CONFIG.nearClip;
    this.camera.farClip = VIEW_CONFIG.farClip;
    await this.renderer.initialize();
    if(this.disposed) {
      this.renderer.destroy();
      return;
    }

    this.debris.synchronizeStructure(this.structure.activeChunks());
    this.rebuildStructureGeometry();
    this.bindInput();
    this.refreshHud();
    document.body.dataset.ready = 'true';
    document.body.dataset.backend = this.renderer.backend ?? 'unknown';
    element('backend').textContent = (this.renderer.backend ?? 'unknown').toUpperCase();
    element('seed').textContent = String(DESTRUCTION_CONFIG.seed);
    window.formDestruction = {
      reset: () => this.reset(),
      snapshot: () => this.snapshot(),
      sample: count => this.sample(count)
    };
    this.animationId = requestAnimationFrame(this.tick);
    if(import.meta.env.DEV) {
      console.info(`FORM destruction ready: ${this.renderer.backend}; seed ${DESTRUCTION_CONFIG.seed}; click a surface to apply damage.`);
    }
  }

  /**
   * Restore chunk health and geometry, discard queued hits, and release all debris.
   */
  reset() {
    this.structure.reset();
    this.debris.reset();
    this.debris.synchronizeStructure(this.structure.activeChunks());
    this.detachedChunks.clear();
    this.impactDebug.clear();
    this.lastImpact = null;
    this.damagePending = false;
    this.rebuildStructureGeometry();
    this.renderer.resetHistory();
    this.refreshHud();
    element('notice').textContent = 'Structure reset';
  }

  /**
   * Return copied diagnostic values. Impact positions use world units; timings use milliseconds.
   */
  snapshot() {
    const memory = (performance as Performance & {memory?: {usedJSHeapSize: number}}).memory;
    return {
      seed: DESTRUCTION_CONFIG.seed,
      backend: this.renderer.backend,
      activeChunks: this.structure.activeChunks().length,
      debris: this.debris.count,
      debrisPhysics: this.debris.snapshot(),
      impactDebug: this.impactDebug.snapshot(),
      support: {enabled: this.debugView === STRUCTURE_VIEW.support, chunks: this.structure.supportState(), detachedChunks: [...this.detachedChunks]},
      integrity: {enabled: this.debugView === STRUCTURE_VIEW.integrity, damagePerHit: this.damagePerHit, chunks: this.structure.damageState()},
      lastImpact: this.lastImpact ? {chunkId: this.lastImpact.chunkId, point: [...this.lastImpact.point]} : null,
      frameTimes: distribution(this.renderSamples),
      damageUpdates: distribution(this.damageSamples),
      impactQueries: distribution(this.querySamples),
      memoryBytes: memory?.usedJSHeapSize ?? null,
      frames: this.frameCount,
      disposed: this.disposed
    };
  }

  /**
   * Collect a bounded number of live frames without changing simulation time.
   */
  async sample(frames: number) {
    if(!Number.isInteger(frames) || frames < 1 || frames > 600) {
      throw new RangeError('Sample frames must be an integer from 1 to 600.');
    }
    const target = this.frameCount + frames;
    while(this.frameCount < target && !this.disposed) {
      await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
    }
    return this.snapshot();
  }

  /**
   * Stop the loop and release input, CPU mesh references, and GPU resources. Safe to repeat.
   */
  dispose() {
    if(this.disposed) {
      return;
    }
    this.disposed = true;
    cancelAnimationFrame(this.animationId);
    this.cancelPointer();
    this.subscriptions.abort();
    this.debris.dispose();
    this.impactDebug.dispose();
    this.renderer.destroy();
    this.scene.destroy();
    this.structureMesh = null;
    delete window.formDestruction;
  }

  /**
   * Project client CSS coordinates onto the nearest active chunk, then queue local damage.
   */
  private impactAt(clientX: number, clientY: number) {
    const ray = this.renderer.screenRay(clientX, clientY);
    if(!ray) {
      return;
    }
    const start = performance.now();
    const hit = this.structure.raycast(ray.origin, ray.direction, DESTRUCTION_CONFIG.maximumAimDistance);
    this.querySamples.push(performance.now() - start);
    retain(this.querySamples);
    this.lastImpact = hit ? {chunkId: hit.chunk.id, point: [...hit.point]} : null;
    if(!hit) {
      element('notice').textContent = 'No structure at this point';
      return;
    }

    this.impactDebug.add(hit.point, ray.direction, this.damageRadius);
    this.structure.queueDamage({center: hit.point, radius: this.damageRadius, damage: this.damagePerHit});
    this.damagePending = true;
  }

  private commitDamage() {
    if(!this.damagePending) {
      return;
    }
    const start = performance.now();
    const changes = this.structure.commit();
    this.damagePending = false;
    const detachedIds = new Set(changes.detached.map(chunk => chunk.id));
    for(const id of detachedIds) {
      this.detachedChunks.add(id);
    }
    this.debris.synchronizeStructure(this.structure.activeChunks());
    this.debris.spawn(changes.removed.filter(chunk => !detachedIds.has(chunk.id)));
    this.debris.spawn(changes.detached, false);
    this.rebuildStructureGeometry();
    this.renderer.resetHistory();
    this.damageSamples.push(performance.now() - start);
    retain(this.damageSamples);
    this.refreshHud();
    if(changes.removed.length) {
      element('notice').textContent = changes.activeChunkCount
        ? `${changes.removed.length - changes.detached.length} broken · ${changes.detached.length} lost support · debris stays until reset` : 'Structure cleared · reset to rebuild';
    } else {
      const target = this.structure.damageState().find(chunk => chunk.chunkId === this.lastImpact?.chunkId);
      const hitsLeft = target ? Math.ceil(target.remainingHitPoints/this.damagePerHit) : 0;
      element('notice').textContent = target
        ? `Wall resisted · ${hitsLeft} more ${hitsLeft === 1 ? 'impact' : 'impacts'} here to break`
        : 'Impact applied';
    }
  }

  private rebuildStructureGeometry() {
    if(this.structureMesh) {
      this.scene.remove(this.structureMesh);
    }
    const asset = new MeshAsset(createStructureMeshData(this.structure, this.debugView));
    this.structureMesh = this.scene.createMesh(asset, {sceneRoughness: 0.72});
  }

  private update(elapsedSeconds: number) {
    this.commitDamage();
    this.debris.update(elapsedSeconds);
    this.impactDebug.update(elapsedSeconds);
  }

  private tick = (timestamp: number) => {
    if(this.disposed) {
      return;
    }
    try {
      const elapsedSeconds = this.previousTimestamp
        ? Math.min((timestamp - this.previousTimestamp)/1000, PRESENTATION_CONFIG.maximumFrameSeconds) : 0;
      this.previousTimestamp = timestamp;
      this.update(elapsedSeconds);

      const renderStart = performance.now();
      this.renderer.render(this.scene, {
        ambient: this.debugView !== STRUCTURE_VIEW.material ? INTEGRITY_CONFIG.ambient : PRESENTATION_CONFIG.ambient,
        clearColor: PRESENTATION_CONFIG.clearColor,
        lights: this.debugView !== STRUCTURE_VIEW.material ? [INTEGRITY_CONFIG.light] : [PRESENTATION_CONFIG.primaryLight, PRESENTATION_CONFIG.secondaryLight],
        timeSeconds: timestamp/1000,
        deltaTimeSeconds: elapsedSeconds
      });
      this.renderSamples.push(performance.now() - renderStart);
      retain(this.renderSamples);
      this.frameCount+= 1;
      if(this.frameCount%PRESENTATION_CONFIG.hudIntervalFrames === 0) {
        this.refreshHud();
      }
      this.animationId = requestAnimationFrame(this.tick);
    } catch(error) {
      showFailure(error);
      this.dispose();
    }
  };

  private bindInput() {
    const signal = this.subscriptions.signal;
    window.addEventListener(EVENT.keydown, event => {
      if(event.code === KEY.escape) {
        element('config-panel').hidden = true;
        this.cancelPointer();
      }
      if(event.code === KEY.reset && !event.repeat && !event.ctrlKey && !event.metaKey && !event.altKey) {
        this.reset();
      }
    }, {signal});
    this.canvas.addEventListener(EVENT.pointerdown, event => {
      if(event.button !== 0 || this.pointer) {
        return;
      }
      this.canvas.focus({preventScroll: true});
      this.pointer = {
        id: event.pointerId, startX: event.clientX, startY: event.clientY,
        x: event.clientX, y: event.clientY, dragged: false
      };
      this.canvas.setPointerCapture(event.pointerId);
    }, {signal});
    this.canvas.addEventListener(EVENT.pointermove, this.movePointer, {signal});
    this.canvas.addEventListener(EVENT.pointerup, event => {
      const pointer = this.pointer;
      if(!pointer || pointer.id !== event.pointerId) {
        return;
      }
      const distance = Math.hypot(event.clientX - pointer.startX, event.clientY - pointer.startY);
      const click = !pointer.dragged && distance <= VIEW_CONFIG.dragThresholdPixels;
      this.cancelPointer();
      if(click) {
        this.impactAt(event.clientX, event.clientY);
      }
    }, {signal});
    for(const type of [EVENT.pointercancel, EVENT.lostpointercapture]) {
      this.canvas.addEventListener(type, event => {
        if(this.pointer?.id === event.pointerId) {
          this.cancelPointer();
        }
      }, {signal});
    }
    window.addEventListener(EVENT.blur, () => this.cancelPointer(), {signal});
    this.canvas.addEventListener(EVENT.wheel, event => {
      event.preventDefault();
      const distance = this.camera.distance*Math.exp(event.deltaY*VIEW_CONFIG.zoomSensitivity);
      this.camera.distance = Math.max(VIEW_CONFIG.minimumDistance, Math.min(VIEW_CONFIG.maximumDistance, distance));
    }, {signal, passive: false});
    element('reset').addEventListener(EVENT.click, () => this.reset(), {signal});
    element('config').addEventListener(EVENT.click, () => {
      const panel = element('config-panel');
      panel.hidden = !panel.hidden;
    }, {signal});
    element('close-config').addEventListener(EVENT.click, () => {element('config-panel').hidden = true;}, {signal});
    const impactDebug = element<HTMLInputElement>('impact-debug');
    impactDebug.checked = this.impactDebug.snapshot().enabled;
    impactDebug.addEventListener(EVENT.input, () => this.impactDebug.setEnabled(impactDebug.checked), {signal});
    const integrityToggle = element<HTMLButtonElement>('integrity-toggle');
    this.refreshIntegrityControls();
    integrityToggle.addEventListener(EVENT.click, () => {
      this.debugView = this.debugView === STRUCTURE_VIEW.integrity ? STRUCTURE_VIEW.material : STRUCTURE_VIEW.integrity;
      this.refreshIntegrityControls();
      this.rebuildStructureGeometry();
      this.renderer.resetHistory();
    }, {signal});
    element('support-toggle').addEventListener(EVENT.click, () => {
      this.debugView = this.debugView === STRUCTURE_VIEW.support ? STRUCTURE_VIEW.material : STRUCTURE_VIEW.support;
      this.refreshIntegrityControls();
      this.rebuildStructureGeometry();
      this.renderer.resetHistory();
    }, {signal});
    const radius = element<HTMLInputElement>('radius');
    radius.value = String(this.damageRadius);
    radius.addEventListener(EVENT.input, () => {this.damageRadius = radius.valueAsNumber;}, {signal});
    const damage = element<HTMLInputElement>('damage');
    damage.value = String(this.damagePerHit);
    damage.addEventListener(EVENT.input, () => {
      this.damagePerHit = damage.valueAsNumber;
      this.refreshHud();
    }, {signal});
    window.addEventListener(EVENT.pagehide, event => {
      if(!event.persisted) {
        this.dispose();
      }
    }, {signal});
  }

  private movePointer = (event: PointerEvent) => {
    const pointer = this.pointer;
    if(!pointer || pointer.id !== event.pointerId) {
      return;
    }
    const distance = Math.hypot(event.clientX - pointer.startX, event.clientY - pointer.startY);
    if(!pointer.dragged && distance <= VIEW_CONFIG.dragThresholdPixels) {
      return;
    }
    // Measure from pointer-down until dragging starts, so small events accumulate correctly.
    pointer.dragged = true;
    this.camera.yaw-= (event.clientX - pointer.x)*VIEW_CONFIG.orbitSensitivity;
    const pitch = this.camera.pitch + (event.clientY - pointer.y)*VIEW_CONFIG.orbitSensitivity;
    this.camera.pitch = Math.max(VIEW_CONFIG.minimumPitch, Math.min(VIEW_CONFIG.maximumPitch, pitch));
    pointer.x = event.clientX;
    pointer.y = event.clientY;
  };

  private cancelPointer() {
    const pointer = this.pointer;
    this.pointer = null;
    if(pointer && this.canvas.hasPointerCapture(pointer.id)) {
      this.canvas.releasePointerCapture(pointer.id);
    }
  }

  private refreshHud() {
    element('chunks').textContent = `${this.structure.activeChunks().length} active`;
    element('debris').textContent = `${this.debris.count} retained`;
    element('integrity-baseline').textContent = `Solid wall: ${Math.ceil(DESTRUCTION_CONFIG.hitPoints/this.damagePerHit)} hits · Frame: ${Math.ceil(DESTRUCTION_CONFIG.frameHitPoints/this.damagePerHit)} hits`;
    const target = this.structure.damageState().find(chunk => chunk.chunkId === this.lastImpact?.chunkId);
    const hitsLeft = target ? Math.ceil(target.remainingHitPoints/this.damagePerHit) : 0;
    element('integrity-target').textContent = target
      ? (target.active ? `${target.remainingHitPoints.toFixed(1)} / ${target.maximumHitPoints} resistance · ${hitsLeft} ${hitsLeft === 1 ? 'hit' : 'hits'} left` : 'Chunk released')
      : 'Click a chunk to inspect its resistance';
    const meter = element<HTMLMeterElement>('integrity-meter');
    meter.max = target?.maximumHitPoints ?? DESTRUCTION_CONFIG.hitPoints;
    meter.value = target && !target.active ? 0 : (target?.remainingHitPoints ?? meter.max);
    const support = this.structure.supportState().find(chunk => chunk.chunkId === this.lastImpact?.chunkId);
    let supportText = `Ground support · at most ${SUPPORT_CONFIG.maximumCantileverDistance.toFixed(2)} m sideways`;
    if(support) {
      if(!support.supported) {
        supportText = this.detachedChunks.has(support.chunkId) ? 'Support lost · detached with health remaining' : 'Broken by impact · no longer part of the wall';
      } else if(support.anchored) {
        supportText = 'Support: anchored to the ground';
      } else {
        supportText = support.lateralDistance ? `Support: braced ${support.lateralDistance.toFixed(2)} m sideways` : 'Support: vertical path to the ground';
      }
    }
    element('support-target').textContent = supportText;
  }

  private refreshIntegrityControls() {
    const toggle = element('integrity-toggle');
    const integrity = this.debugView === STRUCTURE_VIEW.integrity;
    const support = this.debugView === STRUCTURE_VIEW.support;
    toggle.setAttribute('aria-pressed', String(integrity));
    toggle.textContent = integrity ? 'INTEGRITY ON' : 'INTEGRITY';
    element('support-toggle').setAttribute('aria-pressed', String(support));
    element('support-toggle').textContent = support ? 'SUPPORT ON' : 'SUPPORT';
    element('integrity-panel').hidden = this.debugView === STRUCTURE_VIEW.material;
    element('integrity-panel').classList.toggle('support-view', support);
    element('debug-view-title').textContent = support ? 'SUPPORT / GROUND CONNECTION' : 'INTEGRITY / IMPACT RESISTANCE';
    const labels = support ? ['ANCHORED', 'VERTICAL', 'BRACED'] : ['CRITICAL', 'WEAK', 'SOLID', 'FRAME'];
    element('debug-view-labels').replaceChildren(...labels.map(label => {
      const span = document.createElement('span');
      span.textContent = label;
      return span;
    }));
  }
}

function retain(values: number[]) {
  if(values.length > PRESENTATION_CONFIG.timingSampleCount) {
    values.splice(0, values.length - PRESENTATION_CONFIG.timingSampleCount);
  }
}

function distribution(values: readonly number[]) {
  const ordered = [...values].sort((first, second) => first - second);
  return {
    samples: values.length,
    meanMilliseconds: values.reduce((sum, value) => sum + value, 0)/Math.max(values.length, 1),
    p95Milliseconds: ordered[Math.floor((ordered.length - 1)*0.95)] ?? 0,
    maximumMilliseconds: ordered.at(-1) ?? 0
  };
}

function element<T extends HTMLElement = HTMLElement>(id: string): T {
  const value = document.getElementById(id);
  if(!value) {
    throw new Error(`Missing FORM destruction element: ${id}`);
  }
  return value as T;
}

function showFailure(error: unknown) {
  element('failure').hidden = false;
  element('failure-detail').textContent = error instanceof Error ? error.message : String(error);
}

const demo = new FormDestructionDemo();
demo.initialize().catch(error => {
  showFailure(error);
  demo.dispose();
});
if(import.meta.hot) {
  import.meta.hot.dispose(() => demo.dispose());
}
