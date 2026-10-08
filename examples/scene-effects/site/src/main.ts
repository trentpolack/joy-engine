// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT, SCENE_EFFECT, TONE_MAPPER } from 'joy-engine/constants';
import { OrbitCamera, SceneRenderer, gpuBackendFromQuery } from 'joy-engine';
import type { SceneEffectsOptions } from 'joy-engine';
import { createRoom } from '../../src/room.ts';
import { EFFECTS, baselineConfig, qualityConfig } from '../../src/config.ts';
import type { EffectKey, Quality } from '../../src/config.ts';

type Distribution = {samples:number; meanMilliseconds:number; p50Milliseconds:number; p95Milliseconds:number; maximumMilliseconds:number};
export type ExampleSnapshot = ReturnType<SceneEffectsExample['snapshot']>;
export interface SceneEffectsExampleApi {
  setEffect(key:EffectKey, enabled:boolean):void;
  setQuality(quality:Quality):void;
  setBloom(enabled:boolean):void;
  setMotion(objectMotion:boolean, cameraMotion:boolean):void;
  setPaused(paused:boolean):void;
  setTime(seconds:number):void;
  step(seconds?:number):void;
  baseline():void;
  combined():void;
  cameraCut():void;
  replaceScene():void;
  resetHistory():void;
  resizeTest():void;
  snapshot():ExampleSnapshot;
  resetMetrics():void;
  sample(frameCount:number):Promise<ExampleSnapshot>;
  dispose():void;
}
declare global { interface Window {sceneEffectsExample?:SceneEffectsExampleApi;} }

/** Owns one renderer, room, input subscriptions and RAF loop. All simulation clocks use seconds. */
class SceneEffectsExample implements SceneEffectsExampleApi {
  private canvas = element<HTMLCanvasElement>('world');
  private camera = new OrbitCamera();
  private renderer = new SceneRenderer(this.canvas, {
    camera:this.camera, pixelRatio:1, sceneEffects:baselineConfig(),
    device:{backend:gpuBackendFromQuery(location.search), fallbackToWebGl:!location.search.includes('backend=webgpu')},
    postProcessing:{toneMapper:TONE_MAPPER.AGX, agxLook:'neutral', antialiasStrength:0, bloomStrength:0, exposure:1}
  });
  private room = createRoom();
  private subscriptions = new AbortController();
  private enabled = new Set<EffectKey>();
  private quality:Quality = 'medium';
  private bloom = false;
  private paused = true;
  private objectMotion = false;
  private cameraMotion = false;
  private timeSeconds = 2;
  private deltaTimeSeconds = 1/60;
  private cameraOffset = 0;
  private alternateRoom = false;
  private pendingReset = true;
  private disposed = false;
  private animationId = 0;
  private lastTimestamp:number|null = null;
  private frames = 0;
  private cpuSamples:number[] = [];
  private cadenceSamples:number[] = [];
  private pointer:{x:number;y:number}|null = null;

  async initialize() {
    this.camera.target = [0,2,-0.6];
    this.camera.radius = 12;
    this.camera.distance = 20;
    this.camera.nearClip = 0.5;
    this.camera.farClip = 100;
    this.camera.yaw = 0.12;
    this.camera.pitch = 0.25;
    await this.renderer.initialize();
    if(this.disposed) {
      return;
    }
    this.bindControls();
    element('capabilities').textContent = JSON.stringify(this.renderer.sceneEffectsCapabilities, null, 2);
    document.body.dataset.ready = 'true';
    document.body.dataset.backend = this.renderer.backend ?? 'unknown';
    window.sceneEffectsExample = this;
    this.animationId = requestAnimationFrame(this.tick);
    if(import.meta.env.DEV) {
      console.info(`Scene Effects Lab ready: ${this.renderer.backend}; simulation clock frozen at 2 s.`);
    }
  }

  setEffect(key:EffectKey, enabled:boolean) {
    if(!EFFECTS.some(effect => effect.key === key)) {
      throw new RangeError(`Unknown scene effect: ${key}`);
    }
    if(enabled) {
      if(key === SCENE_EFFECT.SSAO) {
        this.enabled.delete(SCENE_EFFECT.GTAO);
      } else if(key === SCENE_EFFECT.GTAO) {
        this.enabled.delete(SCENE_EFFECT.SSAO);
      }
      this.enabled.add(key);
    } else {
      this.enabled.delete(key);
    }
    this.applyEffects();
  }
  setQuality(quality:Quality) {
    if(!['low','medium','high'].includes(quality)) {
      throw new RangeError('Quality must be low, medium or high.');
    }
    this.quality = quality;
    element<HTMLSelectElement>('quality').value = quality;
    this.applyEffects();
  }
  setBloom(enabled:boolean) {
    this.bloom = enabled;
    element<HTMLInputElement>('bloom').checked = enabled;
    this.renderer.setPostProcessingConfig({bloomStrength:enabled ? 0.65 : 0, bloomThreshold:1,
      bloomQuality:this.quality, bloomRadius:0.035});
  }
  setMotion(objectMotion:boolean, cameraMotion:boolean) {
    this.objectMotion = objectMotion;
    this.cameraMotion = cameraMotion;
    element<HTMLInputElement>('objectMotion').checked = objectMotion;
    element<HTMLInputElement>('cameraMotion').checked = cameraMotion;
    this.resetHistory();
  }
  setPaused(paused:boolean) {
    this.paused = paused;
    element<HTMLInputElement>('paused').checked = paused;
    this.lastTimestamp = null;
  }
  setTime(seconds:number) {
    if(!Number.isFinite(seconds) || seconds < 0) {
      throw new RangeError('Simulation time must be finite and nonnegative seconds.');
    }
    this.timeSeconds = seconds;
    element<HTMLInputElement>('time').value = seconds.toFixed(4);
    this.resetHistory();
  }
  step(seconds:number = 1/60) {
    if(!Number.isFinite(seconds) || seconds <= 0) {
      throw new RangeError('Step must be finite positive seconds.');
    }
    this.timeSeconds+= seconds;
    this.deltaTimeSeconds = seconds;
    element<HTMLInputElement>('time').value = this.timeSeconds.toFixed(4);
  }
  baseline() {
    this.enabled.clear();
    this.setBloom(false);
    this.applyEffects();
  }
  combined() {
    this.enabled = new Set([
      SCENE_EFFECT.GTAO,
      SCENE_EFFECT.SSGI,
      SCENE_EFFECT.TAA,
      SCENE_EFFECT.SSR,
      SCENE_EFFECT.CLUSTERED_LIGHTING,
      SCENE_EFFECT.ADAPTIVE_EXPOSURE
    ]);
    this.setBloom(true);
    this.applyEffects();
  }
  cameraCut() {
    this.cameraOffset = this.cameraOffset === 0 ? -0.42 : 0;
    this.camera.yaw = 0.12 + this.cameraOffset;
    this.resetHistory();
  }
  replaceScene() {
    const previous = this.room;
    this.alternateRoom = !this.alternateRoom;
    this.room = createRoom(this.alternateRoom);
    previous.scene.destroy();
    this.resetHistory();
  }
  resetHistory() {
    this.renderer.resetHistory();
    this.pendingReset = true;
  }
  resizeTest() {
    document.body.dataset.compact = document.body.dataset.compact === 'true' ? 'false' : 'true';
    this.resetHistory();
  }
  snapshot() {
    return {
      backend:this.renderer.backend, colorFormat:this.renderer.colorFormat,
      capabilities:this.renderer.sceneEffectsCapabilities,
      viewport:{cssWidth:this.canvas.clientWidth, cssHeight:this.canvas.clientHeight,
        backingWidth:this.canvas.width, backingHeight:this.canvas.height, pixelRatio:this.renderer.pixelRatio},
      settings:{effects:this.effectOptions(), quality:this.quality, bloom:this.bloom,
        paused:this.paused, objectMotion:this.objectMotion, cameraMotion:this.cameraMotion,
        timeSeconds:this.timeSeconds, deltaTimeSeconds:this.deltaTimeSeconds, alternateRoom:this.alternateRoom},
      camera:{target:[...this.camera.target], yaw:this.camera.yaw, pitch:this.camera.pitch, distance:this.camera.distance},
      lights:this.room.lights.length, meshes:this.room.scene.instances.size,
      timing:{cpuRenderSubmission:distribution(this.cpuSamples), rafCadence:distribution(this.cadenceSamples),
        gpuExecution:null, note:'CPU performance.now around renderer.render; RAF timestamp intervals. Neither measures GPU execution.'},
      frames:this.frames
    };
  }
  resetMetrics() {
    this.cpuSamples = [];
    this.cadenceSamples = [];
    this.lastTimestamp = null;
  }
  /** Sample frames with unchanged simulation settings; browser cadence continues while time is frozen. */
  async sample(frameCount:number) {
    if(!Number.isInteger(frameCount) || frameCount < 1 || frameCount > 600) {
      throw new RangeError('Sample count must be an integer from 1 to 600.');
    }
    this.resetMetrics();
    const target = this.frames + frameCount;
    while(this.frames < target && !this.disposed) {
      await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
    }
    return this.snapshot();
  }
  dispose() {
    if(this.disposed) {
      return;
    }
    this.disposed = true;
    cancelAnimationFrame(this.animationId);
    this.subscriptions.abort();
    this.renderer.destroy();
    this.room.scene.destroy();
    if(window.sceneEffectsExample === this) {
      delete window.sceneEffectsExample;
    }
  }

  private effectOptions():SceneEffectsOptions {
    const settings = qualityConfig(this.quality);
    const result = baselineConfig();
    for(const key of this.enabled) {
      // Each key retains its own typed option shape; this loop only selects complete preset entries.
      Object.assign(result, {[key]:settings[key]});
    }
    return result;
  }
  private applyEffects() {
    this.renderer.setSceneEffectsConfig(this.effectOptions());
    for(const effect of EFFECTS) {
      element<HTMLInputElement>(effect.key).checked = this.enabled.has(effect.key);
    }
    this.setBloom(this.bloom);
    this.pendingReset = true;
  }
  private tick = (timestamp:number) => {
    if(this.disposed) {
      return;
    }
    try {
      const interval = this.lastTimestamp === null ? 0 : timestamp - this.lastTimestamp;
      if(interval > 0) {
        retainSample(this.cadenceSamples, interval);
      }
      this.lastTimestamp = timestamp;
      if(!this.paused) {
        this.deltaTimeSeconds = Math.min(interval/1000, 0.05);
        this.timeSeconds+= this.deltaTimeSeconds;
      }
      if(this.objectMotion) {
        this.room.moving.position[0] = Math.sin(this.timeSeconds*1.4)*2.6;
        this.room.moving.rotation[1] = this.timeSeconds*0.9;
      } else {
        this.room.moving.position[0] = 0;
        this.room.moving.rotation[1] = 0;
      }
      if(this.cameraMotion) {
        this.camera.yaw = 0.12 + this.cameraOffset + Math.sin(this.timeSeconds*0.5)*0.2;
      }
      const start = performance.now();
      this.renderer.render(this.room.scene, {lights:this.room.lights, ambient:0.18,
        clearColor:[0.045,0.055,0.08,1], timeSeconds:this.timeSeconds,
        deltaTimeSeconds:this.deltaTimeSeconds, resetHistory:this.pendingReset});
      retainSample(this.cpuSamples, performance.now() - start);
      this.pendingReset = false;
      this.frames+= 1;
      if(this.frames%15 === 0) {
        const cpu = distribution(this.cpuSamples);
        const cadence = distribution(this.cadenceSamples);
        element('stats').textContent = `${this.renderer.backend} · ${this.renderer.colorFormat} · ${this.canvas.width} × ${this.canvas.height}\n` +
          `${this.room.scene.instances.size} meshes / ${this.room.lights.length} lights / t=${this.timeSeconds.toFixed(3)} s\n` +
          `CPU render submission ${cpu.meanMilliseconds.toFixed(2)} ms · RAF cadence ${cadence.meanMilliseconds.toFixed(2)} ms\nGPU execution: unavailable`;
      }
      this.animationId = requestAnimationFrame(this.tick);
    } catch(error) {
      showFailure(error);
      this.dispose();
    }
  };

  private bindControls() {
    const signal = this.subscriptions.signal;
    for(const effect of EFFECTS) {
      const label = document.createElement('label');
      label.className = 'effect';
      const checkbox = document.createElement('input');
      checkbox.type = 'checkbox';
      checkbox.id = effect.key;
      const caption = document.createElement('span');
      caption.textContent = effect.label;
      const detail = document.createElement('small');
      detail.textContent = effect.detail;
      caption.append(detail);
      label.append(checkbox, caption);
      element('effects').append(label);
      checkbox.addEventListener(BROWSER_EVENT.CHANGE, () => this.setEffect(effect.key, checkbox.checked), {signal});
    }
    element('quality').addEventListener(BROWSER_EVENT.CHANGE, () => this.setQuality(element<HTMLSelectElement>('quality').value as Quality), {signal});
    element('bloom').addEventListener(BROWSER_EVENT.CHANGE, () => this.setBloom(element<HTMLInputElement>('bloom').checked), {signal});
    for(const id of ['objectMotion','cameraMotion']) {
      element(id).addEventListener(BROWSER_EVENT.CHANGE, () => this.setMotion(element<HTMLInputElement>('objectMotion').checked,
        element<HTMLInputElement>('cameraMotion').checked), {signal});
    }
    element('paused').addEventListener(BROWSER_EVENT.CHANGE, () => this.setPaused(element<HTMLInputElement>('paused').checked), {signal});
    element('time').addEventListener(BROWSER_EVENT.CHANGE, () => this.setTime(element<HTMLInputElement>('time').valueAsNumber), {signal});
    const actions = {baseline:() => this.baseline(), combined:() => this.combined(), step:() => this.step(),
      cut:() => this.cameraCut(), replace:() => this.replaceScene(), reset:() => this.resetHistory(), resize:() => this.resizeTest()};
    for(const [id, action] of Object.entries(actions)) {
      element(id).addEventListener(BROWSER_EVENT.CLICK, action, {signal});
    }
    this.canvas.addEventListener(BROWSER_EVENT.POINTERDOWN, event => {
      this.pointer = {x:event.clientX, y:event.clientY};
      this.canvas.setPointerCapture(event.pointerId);
      this.canvas.focus();
    }, {signal});
    this.canvas.addEventListener(BROWSER_EVENT.POINTERMOVE, event => {
      if(!this.pointer) {
        return;
      }
      this.camera.yaw-= (event.clientX - this.pointer.x)*0.004;
      this.camera.pitch = Math.max(-0.1, Math.min(1.2, this.camera.pitch + (event.clientY - this.pointer.y)*0.004));
      this.pointer = {x:event.clientX, y:event.clientY};
    }, {signal});
    this.canvas.addEventListener(BROWSER_EVENT.POINTERUP, () => { this.pointer = null; }, {signal});
    this.canvas.addEventListener(BROWSER_EVENT.POINTERCANCEL, () => { this.pointer = null; }, {signal});
    this.canvas.addEventListener(BROWSER_EVENT.WHEEL, event => {
      event.preventDefault();
      this.camera.distance = Math.max(8, Math.min(32, this.camera.distance*Math.exp(event.deltaY*0.001)));
    }, {signal, passive:false});
    window.addEventListener(BROWSER_EVENT.PAGEHIDE, event => {
      if(!event.persisted) {
        this.dispose();
      }
    }, {signal});
  }
}

function element<T extends HTMLElement = HTMLElement>(id:string):T {
  const node = document.getElementById(id);
  if(!node) {
    throw new Error(`Missing scene-effects element: ${id}`);
  }
  return node as T;
}
function retainSample(samples:number[], value:number) {
  samples.push(value);
  if(samples.length > 240) {
    samples.shift();
  }
}
function distribution(samples:number[]):Distribution {
  const ordered = [...samples].sort((a,b) => a - b);
  return {samples:ordered.length, meanMilliseconds:samples.reduce((total,value) => total + value, 0)/Math.max(1,samples.length),
    p50Milliseconds:ordered[Math.floor((ordered.length - 1)*0.5)] ?? 0,
    p95Milliseconds:ordered[Math.floor((ordered.length - 1)*0.95)] ?? 0,
    maximumMilliseconds:ordered.at(-1) ?? 0};
}
function showFailure(error:unknown) {
  const failure = element('failure');
  failure.hidden = false;
  failure.textContent = `Scene Effects Lab could not render.\n${error instanceof Error ? error.message : String(error)}`;
  document.body.dataset.ready = 'false';
}

const example = new SceneEffectsExample();
example.initialize().catch(error => { showFailure(error); example.dispose(); });
if(import.meta.hot) {
  import.meta.hot.dispose(() => example.dispose());
}
