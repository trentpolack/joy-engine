// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import { OrbitCamera, SceneRenderer, RenderScene, MeshAsset, gpuBackendFromQuery } from 'joy-engine';
import type { MeshAssetData } from 'joy-engine';
import { compileFormScript } from 'joy-engine/form';
import type { ParameterValue, NumericParameter } from 'joy-engine/form';
import source from '../../assets/building.form?raw';
import presetData from '../../assets/presets.json';
import { assetFromEvaluation } from '../../src/asset.ts';
import { populateBuildingScene } from '../../src/presentation.ts';

const PRESETS:Record<string,Record<string,ParameterValue>> = presetData;
const MODE_BAKED = new URLSearchParams(location.search).get('mode') === 'baked';
const EVENTS = {change:'change', click:'click', down:'pointerdown', move:'pointermove', up:'pointerup', cancel:'pointercancel', wheel:'wheel', hide:'pagehide'};

declare global {interface Window {buildingExample?:BuildingExample;}}

/** Owns one scene, renderer, parameter snapshot, and RAF loop. FORM generates all asset geometry. */
class BuildingExample {
  private canvas = element<HTMLCanvasElement>('world');
  private camera = new OrbitCamera();
  private renderer = new SceneRenderer(this.canvas, {camera:this.camera, pixelRatio:1,
    device:{backend:gpuBackendFromQuery(location.search), fallbackToWebGl:true}});
  private scene = new RenderScene();
  private program = MODE_BAKED ? null : compileFormScript(source);
  private overrides:Record<string,ParameterValue> = {};
  private controls = new Map<string,HTMLInputElement>();
  private asset:MeshAsset|null = null;
  private subscriptions = new AbortController();
  private pointer:{x:number;y:number}|null = null;
  private animationId = 0;
  private disposed = false;
  private renderSamples:number[] = [];
  private evaluations = 0;
  private evaluationMilliseconds = 0;
  private preparationMilliseconds = 0;

  async initialize() {
    this.camera.yaw = 0.65;
    this.camera.pitch = 0.35;
    this.camera.nearClip = 0.2;
    this.camera.farClip = 300;
    await this.renderer.initialize();
    if(this.disposed) {
      return;
    }
    this.bindControls();
    if(MODE_BAKED) {
      const response = await fetch(new URL('../../assets/cottage.mesh.json', import.meta.url));
      if(!response.ok) {
        throw new Error('Run npm run bake before opening baked mode.');
      }
      const data = await response.json() as MeshAssetData;
      this.replaceAsset(new MeshAsset(data));
      for(const control of this.controls.values()) {
        control.disabled = true;
      }
      element<HTMLSelectElement>('preset').disabled = true;
      element<HTMLButtonElement>('recipe').disabled = true;
    } else {
      this.setPreset('cottage');
    }
    this.fitCamera();
    window.buildingExample = this;
    document.body.dataset.ready = 'true';
    this.animationId = requestAnimationFrame(this.tick);
    if(import.meta.env.DEV) {
      console.info(`FORM building ready: ${this.renderer.backend}; ${MODE_BAKED ? 'baked mesh' : 'editable recipe'}.`);
    }
  }

  setPreset(name:string) {
    if(MODE_BAKED) {
      return;
    }
    if(!Object.hasOwn(PRESETS,name)) {
      throw new RangeError(`Unknown preset: ${name}`);
    }
    this.regenerate(structuredClone(PRESETS[name]));
    element<HTMLSelectElement>('preset').value = name;
    this.fitCamera();
  }

  /** Evaluate a fresh input snapshot; only replace the visible mesh after successful preparation. */
  regenerate(overrides:Record<string,ParameterValue>) {
    if(this.disposed || !this.program) {
      return;
    }
    const start = performance.now();
    const result = this.program.evaluate(overrides);
    const evaluated = performance.now();
    const asset = assetFromEvaluation(result);
    this.replaceAsset(asset);
    this.overrides = structuredClone(overrides);
    this.evaluationMilliseconds = evaluated - start;
    this.preparationMilliseconds = performance.now() - evaluated;
    this.evaluations+= 1;
    for(const parameter of result.parameters) {
      const control = this.controls.get(parameter.name);
      if(control && typeof parameter.value === 'number') {
        control.value = String(parameter.value);
      }
    }
    element('failure').textContent = '';
  }

  frame() {
    this.fitCamera();
  }

  snapshot() {
    return {backend:this.renderer.backend, mode:MODE_BAKED ? 'baked' : 'editable', evaluations:this.evaluations,
      evaluationMilliseconds:this.evaluationMilliseconds, preparationMilliseconds:this.preparationMilliseconds,
      vertices:(this.asset?.geometry.positions.length ?? 0)/3, triangles:(this.asset?.geometry.indices.length ?? 0)/3,
      materialCount:this.asset?.geometry.materials.length ?? 0, instances:this.scene.instances.size,
      bounds:this.asset?.bounds, renderSubmission:distribution(this.renderSamples), disposed:this.disposed};
  }

  dispose() {
    if(this.disposed) {
      return;
    }
    this.disposed = true;
    cancelAnimationFrame(this.animationId);
    this.subscriptions.abort();
    this.renderer.destroy();
    this.scene.destroy();
    this.asset = null;
    if(window.buildingExample === this) {
      delete window.buildingExample;
    }
  }

  private replaceAsset(asset:MeshAsset) {
    // RenderScene owns references, not GPU meshes. Clearing drops the old generation immediately.
    populateBuildingScene(this.scene,asset);
    this.asset = asset;
    this.renderSamples = [];
    this.renderer.resetHistory();
  }

  private fitCamera() {
    if(!this.asset) {
      return;
    }
    const {min,max} = this.asset.bounds;
    this.camera.target = [0,(min[1] + max[1])/2,0];
    this.camera.distance = Math.max(...max.map((value,index) => value - min[index]))*2.3;
  }

  private tick = () => {
    if(this.disposed) {
      return;
    }
    try {
      const start = performance.now();
      this.renderer.render(this.scene, {ambient:0.65, clearColor:[0.065,0.105,0.15,1],
        lights:[{x:8,y:20,z:14,r:4,g:3.6,b:3.1,range:60}], timeSeconds:0, deltaTimeSeconds:1/60});
      this.renderSamples.push(performance.now() - start);
      if(this.renderSamples.length > 120) {
        this.renderSamples.shift();
      }
      element('stats').textContent = JSON.stringify(this.snapshot(),null,2);
      this.animationId = requestAnimationFrame(this.tick);
    } catch(error) {
      showFailure(error);
      this.dispose();
    }
  };

  private bindControls() {
    const signal = this.subscriptions.signal;
    const select = element<HTMLSelectElement>('preset');
    for(const name of Object.keys(PRESETS)) {
      const option = document.createElement('option');
      option.value = name;
      option.textContent = name;
      select.append(option);
    }
    select.addEventListener(EVENTS.change, () => this.setPreset(select.value), {signal});
    for(const parameter of (this.program?.evaluate().parameters ?? [])) {
      if(parameter.type !== 'scalar') {
        continue;
      }
      const numeric = parameter as NumericParameter;
      const label = document.createElement('label');
      label.textContent = parameter.name.replaceAll('_',' ');
      const input = document.createElement('input');
      input.type = 'number';
      input.min = String(numeric.min);
      input.max = String(numeric.max);
      input.step = String(numeric.step);
      input.value = String(numeric.value);
      input.setAttribute('aria-label',parameter.name);
      input.addEventListener(EVENTS.change, () => {
        try {
          this.regenerate({...this.overrides,[parameter.name]:input.valueAsNumber});
        } catch(error) {
          showFailure(error);
        }
      }, {signal});
      this.controls.set(parameter.name,input);
      label.append(input);
      element('parameters').append(label);
    }
    element('frame').addEventListener(EVENTS.click, () => this.frame(), {signal});
    element('recipe').addEventListener(EVENTS.click, () => download('building.formlab', {version:1,name:'Building shell',source,overrides:this.overrides}), {signal});
    element('bake').addEventListener(EVENTS.click, () => download('building.mesh.json',this.asset?.geometry), {signal});
    this.canvas.addEventListener(EVENTS.down, event => {
      const pointer = event as PointerEvent;
      this.pointer = {x:pointer.clientX,y:pointer.clientY};
      this.canvas.setPointerCapture(pointer.pointerId);
    }, {signal});
    this.canvas.addEventListener(EVENTS.move, event => {
      if(!this.pointer) {
        return;
      }
      const pointer = event as PointerEvent;
      this.camera.yaw-= (pointer.clientX - this.pointer.x)*0.004;
      this.camera.pitch = Math.max(0.05,Math.min(1.3,this.camera.pitch + (pointer.clientY - this.pointer.y)*0.004));
      this.pointer = {x:pointer.clientX,y:pointer.clientY};
    }, {signal});
    for(const name of [EVENTS.up,EVENTS.cancel]) {
      this.canvas.addEventListener(name, () => {this.pointer = null;}, {signal});
    }
    this.canvas.addEventListener(EVENTS.wheel, event => {
      event.preventDefault();
      this.camera.distance = Math.max(4,Math.min(150,this.camera.distance*Math.exp((event as WheelEvent).deltaY*0.001)));
    }, {signal,passive:false});
    window.addEventListener(EVENTS.hide, () => this.dispose(), {signal});
  }
}

function element<T extends HTMLElement = HTMLElement>(id:string):T {
  const value = document.getElementById(id);
  if(!value) {
    throw new Error(`Missing building example element: ${id}`);
  }
  return value as T;
}
function distribution(samples:number[]) {
  const ordered = [...samples].sort((a,b) => a - b);
  return {samples:samples.length,meanMilliseconds:samples.reduce((total,value) => total + value,0)/Math.max(1,samples.length),
    p95Milliseconds:ordered[Math.floor((ordered.length - 1)*0.95)] ?? 0,
    note:"CPU render submission; GPU execution is not measured."};
}
function download(name:string,value:unknown) {
  const url = URL.createObjectURL(new Blob([JSON.stringify(value,null,2)],{type:'application/json'}));
  const link = document.createElement('a');
  link.href = url;
  link.download = name;
  link.click();
  URL.revokeObjectURL(url);
}
function showFailure(error:unknown) {
  element('failure').textContent = error instanceof Error ? error.message : String(error);
}
const example = new BuildingExample();
example.initialize().catch(error => {showFailure(error); example.dispose();});
if(import.meta.hot) {
  import.meta.hot.dispose(() => example.dispose());
}
