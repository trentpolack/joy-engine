// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { SCENE_EFFECT } from '../../rendering/scene/constants.ts';
import { BROWSER_EVENT } from '../constants.ts';
import { GPU_BACKEND } from '../../rendering/gpu/constants.ts';
import type {SceneRenderer} from '../../rendering/scene/scene-renderer.ts';
import type {SceneEffectsOptions} from '../../rendering/scene/scene-effects.ts';
import styles from './scene-effects-controls.css?raw';

export interface SceneEffectsControlsOptions {
  /** Borrowed initialized or initializing renderer; the caller remains responsible for disposal. */
  renderer: SceneRenderer;
  /** Borrowed mount point, normally a viewport toolbar or development configuration panel. */
  container: HTMLElement;
  onError?: (message: string) => void;
}

const EFFECTS = [
  {key:SCENE_EFFECT.DEPTH_AWARE_BLUR, label:'Depth-aware blur'},
  {key:SCENE_EFFECT.SSAO, label:'SSAO'},
  {key:SCENE_EFFECT.GTAO, label:'GTAO'},
  {key:SCENE_EFFECT.SSGI, label:'SSGI color bleed'},
  {key:SCENE_EFFECT.OUTLINES, label:'Outlines'},
  {key:SCENE_EFFECT.TAA, label:'Temporal antialiasing'},
  {key:SCENE_EFFECT.MOTION_BLUR, label:'Motion blur'},
  {key:SCENE_EFFECT.SSR, label:'Screen-space reflections'},
  {key:SCENE_EFFECT.HEIGHT_FOG, label:'Screen-height fog approximation'},
  {key:SCENE_EFFECT.CLUSTERED_LIGHTING, label:'Clustered volumetric lighting'},
  {key:SCENE_EFFECT.ADAPTIVE_EXPOSURE, label:'Adaptive exposure'}
] as const;
type EffectKey = typeof EFFECTS[number]['key'];
type Quality = 'low' | 'medium' | 'high';
type Preset = 'off' | 'contact' | 'atmosphere' | 'combined' | 'custom';
const PRESETS: Record<Preset, readonly EffectKey[]> = {
  off:[], contact:[SCENE_EFFECT.GTAO,SCENE_EFFECT.OUTLINES], atmosphere:[SCENE_EFFECT.HEIGHT_FOG,SCENE_EFFECT.CLUSTERED_LIGHTING,SCENE_EFFECT.ADAPTIVE_EXPOSURE],
  combined:[
    SCENE_EFFECT.GTAO,
    SCENE_EFFECT.SSGI,
    SCENE_EFFECT.TAA,
    SCENE_EFFECT.SSR,
    SCENE_EFFECT.CLUSTERED_LIGHTING,
    SCENE_EFFECT.ADAPTIVE_EXPOSURE
  ], custom:[]
};

/**
 * Owns an opt-in development panel and its listeners, never the renderer or document.
 * Start with scene effects disabled; call ready after renderer initialization.
 * Options replace the complete scene-effect config and never write authored asset data.
 */
export class SceneEffectsControls {
  private options: SceneEffectsControlsOptions;
  private root: HTMLDetailsElement;
  private summary: HTMLElement;
  private fields: HTMLFieldSetElement;
  private status: HTMLParagraphElement;
  private presetSelect: HTMLSelectElement;
  private qualitySelect: HTMLSelectElement;
  private bloomCheckbox: HTMLInputElement;
  private checkboxes = new Map<EffectKey, HTMLInputElement>();
  private listeners = new AbortController();
  private enabled = new Set<EffectKey>();
  private quality: Quality = 'medium';
  private bloomStrength = 0.5;
  private initialized = false;
  private destroyed = false;

  constructor(options: SceneEffectsControlsOptions) {
    this.options = options;
    this.root = document.createElement('details');
    this.root.className = 'joy-scene-effects-controls';
    const style = document.createElement('style');
    style.textContent = styles;
    this.root.append(style);
    this.summary = document.createElement('summary');
    this.summary.textContent = 'EFFECTS · OFF';
    this.root.append(this.summary);

    const panel = document.createElement('div');
    panel.className = 'joy-scene-effects-panel';
    const heading = document.createElement('strong');
    heading.textContent = 'Scene effects preview';
    const note = document.createElement('p');
    note.className = 'joy-scene-effects-note';
    note.textContent = 'Temporary viewport settings. Not saved with the level or project.';
    panel.append(heading, note);
    this.fields = document.createElement('fieldset');
    this.fields.disabled = true;
    this.presetSelect = this.select('Preset', [['off','All off'],['contact','Contact + edges'],
      ['atmosphere','Atmosphere'],['combined','Combined'],['custom','Custom']]);
    this.presetSelect.dataset.sceneEffectsPreset = '';
    this.qualitySelect = this.select('Quality', [['low','Low'],['medium','Medium'],['high','High']]);
    this.qualitySelect.value = this.quality;
    this.qualitySelect.dataset.sceneEffectsQuality = '';
    for(const effect of EFFECTS) {
      const checkbox = this.checkbox(effect.label);
      checkbox.dataset.sceneEffect = effect.key;
      this.checkboxes.set(effect.key, checkbox);
      checkbox.addEventListener(BROWSER_EVENT.CHANGE, () => {
        if(checkbox.checked) {
          if(effect.key === SCENE_EFFECT.SSAO) {
            this.enabled.delete(SCENE_EFFECT.GTAO);
          } else if(effect.key === SCENE_EFFECT.GTAO) {
            this.enabled.delete(SCENE_EFFECT.SSAO);
          }
          this.enabled.add(effect.key);
        } else {
          this.enabled.delete(effect.key);
        }
        this.presetSelect.value = 'custom';
        this.apply();
      }, {signal:this.listeners.signal});
    }
    this.bloomCheckbox = this.checkbox('HDR bloom');
    this.bloomCheckbox.dataset.sceneEffectsBloom = '';
    this.bloomCheckbox.addEventListener(BROWSER_EVENT.CHANGE, () => this.apply({bloom:true}), {signal:this.listeners.signal});
    this.presetSelect.addEventListener(BROWSER_EVENT.CHANGE, () => {
      const preset = this.presetSelect.value as Preset;
      if(preset !== 'custom') {
        this.enabled = new Set(PRESETS[preset]);
        this.bloomCheckbox.checked = preset === 'combined';
      }
      this.apply({bloom:preset !== 'custom'});
    }, {signal:this.listeners.signal});
    this.qualitySelect.addEventListener(BROWSER_EVENT.CHANGE, () => {
      this.quality = this.qualitySelect.value as Quality;
      this.apply({quality:true});
    }, {signal:this.listeners.signal});
    this.status = document.createElement('p');
    this.status.className = 'joy-scene-effects-status';
    this.status.setAttribute('role', 'status');
    this.status.textContent = 'Waiting for renderer initialization…';
    panel.append(this.status, this.fields);
    this.root.append(panel);
    options.container.append(this.root);
    this.root.addEventListener('toggle', () => this.positionPanel(panel), {signal:this.listeners.signal});
    window.addEventListener(BROWSER_EVENT.RESIZE, () => this.positionPanel(panel), {signal:this.listeners.signal});
  }

  /** Enable controls after startup; preserve the renderer's existing postprocessing until explicit opt-in. */
  ready() {
    if(this.destroyed) {
      return;
    }
    this.initialized = true;
    const display = this.options.renderer.postProcessingConfig;
    if(display) {
      this.bloomCheckbox.checked = display.bloomStrength > 0;
      this.bloomStrength = display.bloomStrength > 0 ? display.bloomStrength : 0.5;
      this.quality = display.bloomQuality === 'ultra' ? 'high' : display.bloomQuality;
      this.qualitySelect.value = this.quality;
      if(this.bloomCheckbox.checked) {
        this.presetSelect.value = 'custom';
      }
    }
    this.fields.disabled = false;
    this.updateStatus();
  }

  /** Keep asynchronous renderer and startup errors visible within the owning viewport panel. */
  reportError(error: unknown) {
    if(this.destroyed) {
      return;
    }
    const message = `Scene effects unavailable: ${error instanceof Error ? error.message : String(error)}`;
    if(this.status.textContent !== message) {
      this.status.textContent = message;
      this.status.dataset.error = 'true';
      this.options.onError?.(message);
    }
  }

  /** Remove owned DOM and listeners once; borrowed render state remains the caller's responsibility. */
  destroy() {
    if(this.destroyed) {
      return;
    }
    this.destroyed = true;
    this.listeners.abort();
    this.root.remove();
  }

  private apply(changes: {bloom?: boolean; quality?: boolean} = {}) {
    if(!this.initialized || this.destroyed) {
      return;
    }
    for(const [key, checkbox] of this.checkboxes) {
      checkbox.checked = this.enabled.has(key);
    }
    try {
      this.options.renderer.setSceneEffectsConfig(this.configuration());
      // Scene toggles never alter the consumer's exposure, display transform or bloom tuning.
      if(changes.bloom || changes.quality) {
        const display = {
          ...this.options.renderer.postProcessingConfig,
          ...(changes.bloom ? {bloomStrength:this.bloomCheckbox.checked ? this.bloomStrength : 0} : {}),
          ...(changes.quality ? {bloomQuality:this.quality} : {})
        };
        this.options.renderer.setPostProcessingConfig(display);
      }
      this.options.renderer.resetHistory();
      this.updateStatus();
    } catch(error) {
      this.reportError(error);
    }
  }

  private positionPanel(panel: HTMLElement) {
    if(!this.root.open || this.destroyed) {
      return;
    }
    const bounds = this.root.getBoundingClientRect();
    let left = 8;
    let right = window.innerWidth - 8;
    let top = 8;
    let bottom = window.innerHeight - 8;
    // A small telemetry mount may overflow freely; only actual clipping ancestors constrain the popup.
    for(let ancestor: HTMLElement | null = this.options.container; ancestor; ancestor = ancestor.parentElement) {
      const style = getComputedStyle(ancestor);
      const rectangle = ancestor.getBoundingClientRect();
      if(/hidden|clip|scroll|auto/.test(style.overflowX)) {
        left = Math.max(left, rectangle.left + 8);
        right = Math.min(right, rectangle.right - 8);
      }
      if(/hidden|clip|scroll|auto/.test(style.overflowY)) {
        top = Math.max(top, rectangle.top + 8);
        bottom = Math.min(bottom, rectangle.bottom - 8);
      }
    }
    const below = bottom - bounds.bottom - 8;
    const above = bounds.top - top - 8;
    const upward = below < Math.min(panel.scrollHeight, 360) && above > below;
    this.root.dataset.placement = upward ? 'above' : 'below';
    panel.style.setProperty('--joy-scene-effects-room', `${Math.max(140, upward ? above : below)}px`);
    const width = Math.min(260, Math.max(80, right - left));
    panel.style.width = `${width}px`;
    panel.style.left = `${Math.max(left - bounds.left,
      Math.min(bounds.width - width, right - bounds.left - width))}px`;
    panel.style.right = 'auto';
  }

  private configuration(): SceneEffectsOptions {
    const resolutionScale = this.quality === 'high' ? 1 : 0.5;
    const config: SceneEffectsOptions = {
      depthAwareBlur:false, ssao:false, gtao:false, ssgi:false, outlines:false, taa:false,
      motionBlur:false, ssr:false, heightFog:false, clusteredLighting:false, adaptiveExposure:false
    };
    for(const key of this.enabled) {
      Object.assign(config, {[key]:true});
    }
    if(this.enabled.has(SCENE_EFFECT.SSAO)) {
      config.ssao = {resolutionScale};
    }
    if(this.enabled.has(SCENE_EFFECT.GTAO)) {
      config.gtao = {resolutionScale};
    }
    if(this.enabled.has(SCENE_EFFECT.SSGI)) {
      config.ssgi = {resolutionScale, rayCount:this.quality === 'low' ? 3 : this.quality === 'high' ? 12 : 7};
    }
    if(this.enabled.has(SCENE_EFFECT.SSR)) {
      config.ssr = {resolutionScale, sampleCount:this.quality === 'low' ? 16 : this.quality === 'high' ? 80 : 48};
    }
    if(this.enabled.has(SCENE_EFFECT.CLUSTERED_LIGHTING)) {
      config.clusteredLighting = {resolutionScale, sampleCount:this.quality === 'low' ? 5 : this.quality === 'high' ? 16 : 10};
    }
    return config;
  }

  private updateStatus() {
    const renderer = this.options.renderer;
    const count = this.enabled.size + Number(this.bloomCheckbox.checked);
    this.summary.textContent = count ? `EFFECTS · ${count} REQUESTED` : 'EFFECTS · OFF';
    this.status.dataset.error = 'false';
    if(renderer.backend === GPU_BACKEND.WEBGL) {
      this.status.textContent = 'WebGL 2: scene effects require WebGPU and are unavailable here. HDR bloom remains available.';
      return;
    }
    const report = renderer.sceneEffectsCapabilities;
    this.status.textContent = `WebGPU · ${renderer.colorFormat ?? 'display color'} · ${count ? `${this.enabled.size} scene effects requested` : 'scene effects off'}.`;
    if(report && !report.available) {
      this.status.textContent = 'Scene effects are unavailable on this renderer. See device capabilities.';
    }
  }

  private select(caption: string, choices: readonly (readonly [string, string])[]) {
    const label = document.createElement('label');
    label.className = 'joy-scene-effects-select';
    label.append(document.createTextNode(caption));
    const select = document.createElement('select');
    select.setAttribute('aria-label', `Scene effects ${caption.toLowerCase()}`);
    for(const [value, text] of choices) {
      const option = document.createElement('option');
      option.value = value;
      option.textContent = text;
      select.append(option);
    }
    label.append(select);
    this.fields.append(label);
    return select;
  }

  private checkbox(caption: string) {
    const label = document.createElement('label');
    label.className = 'joy-scene-effects-toggle';
    const checkbox = document.createElement('input');
    checkbox.type = 'checkbox';
    label.append(checkbox, document.createTextNode(caption));
    this.fields.append(label);
    return checkbox;
  }
}
