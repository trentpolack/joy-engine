// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT, KEY_CODE, PARTICLE_PARAMETER_TYPE, PARTICLE_RENDERER } from 'joy-engine/constants';
import { PARTICLE_PROFILE_MODE } from '../../src/constants.ts';
import type { ParticleEmitterAsset, ParticleEffectAsset } from 'joy-engine';
import {PanelLayout} from '../../../../site/src/interaction/panel-layout.ts';
import {NumericControls} from '../../../../site/src/interaction/numeric-controls.ts';
import {guardNumberScroll} from '../../../../site/src/interaction/number-scroll.ts';
import { hosted, workspaceActive, connectWorkspaceTool, requestWorkspaceMaximize } from '../../../../site/src/workspace/tool-bridge.ts';
import { version } from '../../package.json';
import { mountEditorNavigation } from '../../../../site/src/navigation.ts';
import '@fontsource/archivo/400.css';
import '@fontsource/archivo/600.css';
import '@fontsource/archivo-black';
import { compileParticleEffect, ParticleEffect, PARTICLE_STEP_SECONDS, DEFAULT_PARTICLE_TEXTURE } from 'joy-engine';
import { downloadAsset, MAX_ASSET_BYTES, readAsset, writeAsset } from '../../src/document.ts';
import { copyExample, createEmitterTemplate, EXAMPLES, EMITTER_PRESETS } from '../../src/examples.ts';
import { readStoredText, storeText } from '../../src/storage.ts';
import { ParticleScriptEditor } from '../../src/editor.ts';
import { ParticleViewport } from '../../src/viewport.ts';
import type { PreviewPerformanceMode } from '../../src/viewport.ts';
import { parameterMarkup, setParameterCollapsed, refreshRangeFill, colorStyle } from '../../src/parameter-controls.ts';

const STORAGE_KEY = 'joy-games:particle-lab:v1';
const LIVE_DELAY_MILLISECONDS = 260;
const PREVIEW_PENDING = 'pending';
const SEEK_STEPS_PER_FRAME = 120;
const FULL_REPLAY = 'full';
const CUSTOM_REPLAY = 'custom';
const MAX_REPLAY_SECONDS = 30;
const PERFORMANCE_REFRESH_MILLISECONDS = 250;

/** Owns document state, simulation, controls, and preview lifetime. */
class ParticleLab {
  declare asset: ParticleEffectAsset;
  declare effect: ParticleEffect | null;
  declare collapsedParameters: Set<string>;
  declare selectedEmitter: number;
  declare playing: boolean;
  declare lastFrameMilliseconds: number;
  declare frameRequest: number;
  declare applyTimer: number;
  declare applyGeneration: number;
  declare seekGeneration: number;
  declare disposed: boolean;
  declare scrubbing: boolean;
  declare seed: number;
  declare speed: number;
  declare duration: number;
  declare replayWindowSeconds: number;
  declare previewFps: number;
  declare simulationMilliseconds: number;
  declare telemetryTimestamp: number;
  declare telemetryFrames: number;
  declare profileSteps: number;
  declare emitterSamples: Map<string, { spawnMs: number; updateMs: number; spawnSample: number; updateSample: number; }>;
  declare performanceMode: PreviewPerformanceMode;
  declare elements: ReturnType<typeof resolveElements>;
  declare viewport: ParticleViewport;
  declare listeners: AbortController;
  declare panelLayout: PanelLayout;
  declare numeric: NumericControls;
  declare textureUploadGeneration: number;
  declare editors: { spawn: ParticleScriptEditor; update: ParticleScriptEditor; };

  constructor() {
    get('tool-version').textContent = version;

    this.asset = loadInitialAsset();

    this.effect = null;

    /** Document-local UI state; excluded from exported effect data. */
    this.collapsedParameters = new Set();
    this.selectedEmitter = 0;
    this.playing = true;
    this.lastFrameMilliseconds = performance.now();
    this.frameRequest = 0;
    this.applyTimer = 0;
    this.applyGeneration = 0;
    this.seekGeneration = 0;
    this.disposed = false;
    this.scrubbing = false;
    this.seed = 7777;
    this.speed = 1;
    this.duration = 6;
    this.replayWindowSeconds = 0;
    this.previewFps = 0;
    this.simulationMilliseconds = 0;
    this.telemetryTimestamp = 0;
    this.telemetryFrames = 0;
    this.profileSteps = 0;
    this.performanceMode = 'overview';

    this.emitterSamples = new Map();

    this.elements = resolveElements();
    this.viewport = new ParticleViewport(this.elements.canvas, message => this.showGpuError(message));
    this.listeners = new AbortController();
    guardNumberScroll(document, this.listeners.signal);
    this.panelLayout = new PanelLayout(((document.querySelector('.workspace')) as HTMLElement), {left:  ((document.querySelector('.inspector-panel')) as HTMLElement), right:  ((document.querySelector('.library-panel')) as HTMLElement), center:  ((document.querySelector('.stage')) as HTMLElement), key: 'joy-editor-panels-particles-v2', leftWidth: 320, rightWidth: 260, maximizeMode: 'workspace', onMaximize: requestWorkspaceMaximize});
    this.numeric = new NumericControls(get('app'), this.listeners.signal, {
      selector: '#parameter-list input, #emitter-tab input',
      read: () => this.propertySnapshot(),
      commit: input => {
        if(input.closest('#parameter-list')) {
          this.editParameterInput(input, 'change');
        } else {
          this.editEmitterInput(input);
        }
      },
      restore: text => this.restoreProperties(text),
      changed: () => {
        get('property-undo', HTMLButtonElement).disabled = !this.numeric?.past.length;
        get('property-redo', HTMLButtonElement).disabled = !this.numeric?.future.length;
      }
    });
    this.textureUploadGeneration = 0;
    this.editors = {
      spawn: new ParticleScriptEditor(this.elements.spawn, 'Spawn script editor', false, () => {
        this.numeric.clear();
        this.currentEmitter().spawn = this.editors.spawn.value;
        this.documentChanged();
      }, () => this.apply()),
      update: new ParticleScriptEditor(this.elements.update, 'Update script editor', false, () => {
        this.numeric.clear();
        this.currentEmitter().update = this.editors.update.value;
        this.documentChanged();
      }, () => this.apply()),
    };
  }

  /** Property-only snapshots exclude script text and embedded texture bytes. */
  propertySnapshot() {
    return JSON.stringify({parameters:this.asset.parameters, emitters:this.asset.emitters.map(({spawn,update,texture,...properties}) => ({...properties, ...(texture ? {texture: (({source,...settings}) => settings)(texture)} : {})}))});
  }

  /** Restore through the existing asset validator, preserving borrowed scripts and textures.
   * @param text
   */
  restoreProperties(text: string) {
    const properties = ((JSON.parse(text)) as ParticleEffectAsset);
    const candidate = structuredClone(this.asset);
    candidate.parameters = properties.parameters;
    candidate.emitters = properties.emitters.map((emitter,index) => ({...emitter, spawn:candidate.emitters[index].spawn, update:candidate.emitters[index].update,
      ...(emitter.texture ? {texture:{...emitter.texture, source:candidate.emitters[index].texture?.source ?? DEFAULT_PARTICLE_TEXTURE}} : {})}));
    this.asset = readAsset(writeAsset(candidate));
    this.refreshAll();
    this.documentChanged();
  }

  async initialize() {
    this.populateExamples();
    this.bind();
    this.refreshAll();
    this.apply();

    try {
      const backend = await this.viewport.initialize();
      this.elements.backend.textContent = (backend || 'GPU').toUpperCase().replace('WEBGL', 'WEBGL 2');
    } catch(error) {
      this.showGpuError(error instanceof Error ? error.message : String(error));
    }

    this.lastFrameMilliseconds = performance.now();
    this.frameRequest = requestAnimationFrame(timestamp => this.frame(timestamp));
  }

  bind() {
    const signal = this.listeners.signal;
    const updatePreview = () => this.viewport.setPreviewOptions({
      gridVisible: get('grid-toggle', HTMLInputElement).checked,
      receiverVisible: get('light-receiver', HTMLInputElement).checked,
      background: get('preview-background', HTMLSelectElement).value,
    });
    updatePreview();
    for(const id of ['grid-toggle', 'light-receiver', 'preview-background']) {
      get(id).addEventListener(BROWSER_EVENT.CHANGE, updatePreview, { signal });
    }
    get('frame-effect').addEventListener(BROWSER_EVENT.CLICK, () => this.viewport.frameEffect(), { signal });
    get('reset-camera').addEventListener(BROWSER_EVENT.CLICK, () => this.viewport.resetCamera(), { signal });
    get('apply').addEventListener(BROWSER_EVENT.CLICK, () => this.apply(), { signal });
    this.elements.live.addEventListener(BROWSER_EVENT.CHANGE, () => {
      clearTimeout(this.applyTimer);
      if(this.elements.live.checked && this.elements.diagnostic.dataset.state === PREVIEW_PENDING) {
        this.scheduleApply();
      }
    }, {signal});
    get('save').addEventListener(BROWSER_EVENT.CLICK, () => {
      if(!this.numeric.validate(false)) {
        return;
      }
      this.numeric.capture();
      downloadAsset(this.asset.name, writeAsset(this.asset));
    }, { signal });
    get('open').addEventListener(BROWSER_EVENT.CLICK, () => this.elements.file.click(), { signal });
    this.elements.file.addEventListener(BROWSER_EVENT.CHANGE, () => this.openFile(), { signal });
    this.elements.textureFile.addEventListener(BROWSER_EVENT.CHANGE, () => this.uploadTexture(), { signal });
    get('help').addEventListener(BROWSER_EVENT.CLICK, () => {
      get('reference-dialog', HTMLDialogElement).showModal();
    }, { signal });
    get('close-reference').addEventListener(BROWSER_EVENT.CLICK, () => {
      get('reference-dialog', HTMLDialogElement).close();
    }, { signal });
    get('performance-mode').addEventListener(BROWSER_EVENT.CHANGE, () => this.syncPerformanceMode(), { signal });

    get('add-emitter').addEventListener(BROWSER_EVENT.CLICK, () => {
      if(this.numeric.validate(false)) {
        get('emitter-chooser', HTMLDialogElement).showModal();
      }
    }, {signal});
    get('cancel-emitter').addEventListener(BROWSER_EVENT.CLICK, () => get('emitter-chooser', HTMLDialogElement).close(), {signal});
    for(const preset of EMITTER_PRESETS) {
      const button = document.createElement('button');
      button.type = 'button';
      button.dataset.preset = preset.id;
      button.textContent = preset.label;
      const description = document.createElement('small');
      description.textContent = preset.description;
      button.append(description);
      button.addEventListener(BROWSER_EVENT.CLICK, () => {
        this.addEmitter(preset.id);
        get('emitter-chooser', HTMLDialogElement).close();
      }, {signal});
      get('emitter-presets').append(button);
    }
    get('duplicate-emitter').addEventListener(BROWSER_EVENT.CLICK, () => this.duplicateEmitter(), { signal });
    get('delete-emitter').addEventListener(BROWSER_EVENT.CLICK, () => this.deleteEmitter(), { signal });
    get('add-parameter').addEventListener(BROWSER_EVENT.CLICK, () => this.addParameter(), { signal });

    get('property-undo').addEventListener(BROWSER_EVENT.CLICK, () => this.numeric.undo(), {signal});
    get('property-redo').addEventListener(BROWSER_EVENT.CLICK, () => this.numeric.redo(), {signal});
    const updateReplay = () => {
      const mode = get('replay-window', HTMLSelectElement).value;
      const custom = get('replay-custom', HTMLInputElement);
      custom.hidden = mode !== CUSTOM_REPLAY;
      const seconds = mode === FULL_REPLAY ? 0 : mode === CUSTOM_REPLAY ? custom.valueAsNumber : Number(mode);
      if(!Number.isFinite(seconds) || seconds < 0 || seconds > MAX_REPLAY_SECONDS || (mode === CUSTOM_REPLAY && seconds <= 0)) {
        custom.setCustomValidity('Enter a duration greater than 0 and at most 30 seconds.');
        custom.reportValidity();
        return;
      }
      custom.setCustomValidity('');
      this.replayWindowSeconds = seconds;
      this.refreshTimeline();
      this.restart();
    };
    get('replay-window').addEventListener(BROWSER_EVENT.CHANGE, updateReplay, {signal});
    get('replay-custom').addEventListener(BROWSER_EVENT.CHANGE, updateReplay, {signal});
    get('restart').addEventListener(BROWSER_EVENT.CLICK, () => this.restart(), { signal });
    this.elements.play.addEventListener(BROWSER_EVENT.CLICK, () => this.togglePlay(), { signal });
    get('step').addEventListener(BROWSER_EVENT.CLICK, () => this.step(), { signal });
    this.elements.seed.addEventListener(BROWSER_EVENT.CHANGE, () => {
      this.seed = Math.trunc(this.elements.seed.valueAsNumber) || 0;
      this.apply();
    }, { signal });
    this.elements.speed.addEventListener(BROWSER_EVENT.CHANGE, () => {
      this.speed = Number(this.elements.speed.value);
    }, { signal });

    this.elements.timeline.addEventListener(BROWSER_EVENT.POINTERDOWN, () => {
      this.scrubbing = true;
    }, { signal });
    document.addEventListener(BROWSER_EVENT.POINTERUP, () => this.releaseTimeline(), { signal });
    document.addEventListener(BROWSER_EVENT.POINTERCANCEL, () => this.releaseTimeline(), { signal });
    this.elements.timeline.addEventListener(BROWSER_EVENT.INPUT, () => {
      void this.seek(this.elements.timeline.valueAsNumber);
    }, { signal });

    this.elements.examples.addEventListener(BROWSER_EVENT.CHANGE, () => {
      if(!this.numeric.validate(false)) {
        return;
      }
      this.numeric.clear();
      this.asset = copyExample(Number(this.elements.examples.value));
      this.collapsedParameters.clear();
      this.selectedEmitter = 0;
      this.refreshAll();
      this.apply();
    }, { signal });
    this.elements.name.addEventListener(BROWSER_EVENT.INPUT, () => {
      const intendedName = this.elements.name.value;
      this.asset.name = intendedName;
      this.elements.name.value = intendedName;
      this.documentChanged();
    }, { signal });
    this.elements.emitterList.addEventListener(BROWSER_EVENT.CLICK, event => {
      const target = ((event.target) as HTMLElement);
      const button = target.closest('button[data-index]');
      if(button instanceof HTMLButtonElement) {
        if(!this.numeric.validate(false)) {
          return;
        }
        this.selectedEmitter = Number(button.dataset.index);
        this.refreshInspector();
        this.refreshEmitters();
      }
    }, { signal });
    this.elements.parameterList.addEventListener(BROWSER_EVENT.CHANGE, event => this.editParameter(event), { signal });
    this.elements.parameterList.addEventListener(BROWSER_EVENT.INPUT, event => this.editParameter(event), { signal });
    this.elements.parameterList.addEventListener(BROWSER_EVENT.CLICK, event => this.editParameter(event), { signal });
    this.elements.emitterTab.addEventListener(BROWSER_EVENT.INPUT, event => this.editEmitter(event), { signal });
    this.elements.emitterTab.addEventListener(BROWSER_EVENT.CLICK, event => this.clickEmitterInspector(event), { signal });

    for(const button of document.querySelectorAll('.tabs button')) {
      button.addEventListener(BROWSER_EVENT.CLICK, () => {
        const tab = ((button) as HTMLButtonElement).dataset.tab ?? 'emitter';
        this.selectTab(tab);
      }, { signal });
    }

    document.addEventListener(BROWSER_EVENT.KEYDOWN, event => {
      if(!event.defaultPrevented && (event.ctrlKey || event.metaKey) && event.key === KEY_CODE.ENTER) {
        event.preventDefault();
        this.apply();
      } else if(event.key === ' ' && !isEditing(event.target)) {
        event.preventDefault();
        this.togglePlay();
      } else if(event.key.toLowerCase() === 'r' && !isEditing(event.target)) {
        this.restart();
      }
    }, { signal });
    window.addEventListener(BROWSER_EVENT.PAGEHIDE, event => {
      if(!event.persisted) {
        this.destroy();
      }
    }, { signal });
  }

  /**
   * Apply the current valid definition; automatic requests obey Live through completion.
   * Explicit Apply remains available regardless of the Live preference.
   * @param [automatic]
   */
  async apply(automatic: boolean = false) {
    if(automatic && !this.elements.live.checked) {
      return;
    }
    if(!this.numeric.validate(false)) {
      return;
    }
    const generation = ++this.applyGeneration;
    clearTimeout(this.applyTimer);
    this.seekGeneration += 1;
    const previousAsset = this.asset;

    let pendingEffect: ParticleEffect | null = null;

    try {
      const candidate = structuredClone(this.asset);
      const compiled = compileParticleEffect(candidate);
      const parameters = Object.fromEntries(
        Object.entries(candidate.parameters).map(([name, parameter]) => [name, parameter.value]),
      );
      const nextEffect = new ParticleEffect(compiled, { seed: this.seed, parameters, profiling: this.performanceMode === PARTICLE_PROFILE_MODE.DETAILED });
      pendingEffect = nextEffect;
      nextEffect.step();
      const ready = await this.viewport.prepareEffect(nextEffect);
      if(!ready || generation !== this.applyGeneration || this.disposed || (automatic && !this.elements.live.checked)) {
        nextEffect.destroy();
        return;
      }

      const previousEffect = this.effect;
      this.asset = structuredClone(candidate);
      this.effect = nextEffect;
      // Preparation can finish after the author changes the monitoring mode.
      this.syncEffectProfiling(nextEffect);
      this.simulationMilliseconds = 0;
      this.telemetryFrames = 0;
      this.emitterSamples.clear();
      this.profileSteps = 0;
      pendingEffect = null;
      this.selectedEmitter = Math.min(this.selectedEmitter, this.asset.emitters.length - 1);
      this.duration = calculateDuration(this.asset);
      this.viewport.setEffect(nextEffect);
      previousEffect?.destroy();

      this.refreshTimeline();
      this.hideError();
      if(!hosted) {
        storeText(STORAGE_KEY, writeAsset(this.asset));
      }
      this.elements.diagnostic.textContent = 'LIVE';
      this.elements.diagnostic.dataset.state = 'live';
    } catch(error) {
      pendingEffect?.destroy();
      if(generation === this.applyGeneration && !this.disposed) {
        this.asset = previousAsset;
        this.showError(error instanceof Error ? error.message : String(error));
      }
    }
  }

  /** @param timestampMilliseconds */
  frame(timestampMilliseconds: number) {
    if(this.disposed) {
      return;
    }

    if(hosted && !workspaceActive()) {
      this.lastFrameMilliseconds = timestampMilliseconds;
      this.frameRequest = requestAnimationFrame(timestamp => this.frame(timestamp));
      return;
    }
    const elapsedSeconds = Math.max(0, (timestampMilliseconds - this.lastFrameMilliseconds) / 1000);
    this.lastFrameMilliseconds = timestampMilliseconds;
    const monitoring = this.performanceMode !== PARTICLE_PROFILE_MODE.DISABLED;
    if(monitoring && elapsedSeconds > 0) {
      const fps = 1 / elapsedSeconds;
      this.previewFps = this.previewFps ? this.previewFps * 0.9 + fps * 0.1 : fps;
    }
    if(this.playing && this.effect && !this.scrubbing) {
      const simulationStart = monitoring ? performance.now() : 0;
      try {
        let remainingSeconds = Math.min(0.25, elapsedSeconds) * this.speed;
        while(remainingSeconds > 0) {
          const sliceSeconds = Math.min(0.25, remainingSeconds);
          this.effect.update(sliceSeconds);
          remainingSeconds -= sliceSeconds;
        }

        const reachedWindow = this.replayWindowSeconds > 0 && this.effect.time >= this.replayWindowSeconds;
        if(reachedWindow && !this.elements.loop.checked) {
          this.playing = false;
          this.syncPlayButton();
        }
        if(this.elements.loop.checked && (this.replayWindowSeconds > 0 ? reachedWindow : !this.effect.isAlive)) {
          this.effect.reset();
          this.emitterSamples.clear();
          this.profileSteps = 0;
        }
        if(monitoring) {
          this.simulationMilliseconds+= performance.now() - simulationStart;
        }
        this.viewport.render();
      } catch(error) {
        this.playing = false;
        this.syncPlayButton();
        this.showError(error instanceof Error ? error.message : String(error));
      }
    }

    if(monitoring) {
      this.telemetryFrames+= 1;
      if(timestampMilliseconds - this.telemetryTimestamp >= PERFORMANCE_REFRESH_MILLISECONDS) {
        this.refreshPerformance();
        this.telemetryTimestamp = timestampMilliseconds;
      }
    }
    this.refreshDiagnostics();
    this.frameRequest = requestAnimationFrame(timestamp => this.frame(timestamp));
  }

  restart() {
    this.seekGeneration += 1;
    this.effect?.reset();
    this.emitterSamples.clear();
    this.profileSteps = 0;
    this.playing = true;
    this.syncPlayButton();
    this.viewport.render();
  }

  step() {
    this.seekGeneration += 1;
    this.playing = false;
    this.syncPlayButton();
    try {
      this.effect?.step();
      this.viewport.render();
      this.refreshDiagnostics();
    } catch(error) {
      this.showError(error instanceof Error ? error.message : String(error));
    }
  }

  togglePlay() {
    this.playing = !this.playing;
    this.syncPlayButton();
  }

  /** Incrementally seek without holding the main thread for a long simulation. @param seconds */
  async seek(seconds: number) {
    const effect = this.effect;
    if(!effect) {
      return;
    }

    const generation = ++this.seekGeneration;
    const resumePlayback = this.playing;
    this.playing = false;
    this.syncPlayButton();
    effect.reset();
    this.emitterSamples.clear();
    this.profileSteps = 0;
    let remainingSteps = Math.round(Math.min(30, Math.max(0, seconds)) / PARTICLE_STEP_SECONDS);

    try {
      while(remainingSteps > 0 && generation === this.seekGeneration && !this.disposed) {
        const stepCount = Math.min(SEEK_STEPS_PER_FRAME, remainingSteps);
        for(let index = 0; index < stepCount; index += 1) {
          effect.step();
        }
        remainingSteps -= stepCount;
        this.viewport.render();
        this.refreshDiagnostics();
        if(remainingSteps > 0) {
          await nextAnimationFrame();
        }
      }
    } catch(error) {
      this.showError(error instanceof Error ? error.message : String(error));
    } finally {
      if(generation === this.seekGeneration) {
        this.playing = resumePlayback;
        this.syncPlayButton();
      }
    }
  }

  releaseTimeline() {
    this.scrubbing = false;
  }

  syncPlayButton() {
    this.elements.play.textContent = this.playing ? 'Ⅱ Pause' : '▶ Play';
    this.elements.play.setAttribute('aria-pressed', String(this.playing));
  }

  documentChanged() {
    if(!hosted) {
      storeText(STORAGE_KEY, writeAsset(this.asset));
    }
    this.scheduleApply();
  }

  scheduleApply() {
    // New edits cancel the older timer even when automatic preview is disabled.
    clearTimeout(this.applyTimer);
    this.applyGeneration += 1;
    this.elements.diagnostic.textContent = 'PENDING CHANGES';
    this.elements.diagnostic.dataset.state = PREVIEW_PENDING;
    if(this.elements.live.checked) {
      this.applyTimer = window.setTimeout(() => this.apply(true), LIVE_DELAY_MILLISECONDS);
    }
  }

  refreshAll() {
    this.elements.name.value = this.asset.name;
    this.refreshEmitters();
    this.refreshParameters();
    this.refreshInspector();
    this.duration = calculateDuration(this.asset);
    this.refreshTimeline();
    this.syncPerformanceMode();
  }

  syncPerformanceMode() {
    const mode = get('performance-mode', HTMLSelectElement).value as PreviewPerformanceMode;
    get('performance').hidden = mode === PARTICLE_PROFILE_MODE.DISABLED;
    get('performance-details').hidden = mode !== PARTICLE_PROFILE_MODE.DETAILED;
    if(this.performanceMode === mode) {
      return;
    }
    this.performanceMode = mode;
    this.simulationMilliseconds = 0;
    this.telemetryFrames = 0;
    this.telemetryTimestamp = this.lastFrameMilliseconds;
    this.previewFps = 0;
    this.emitterSamples.clear();
    this.profileSteps = 0;
    get('emitter-performance').replaceChildren();
    if(this.effect) {
      this.syncEffectProfiling(this.effect);
    }
    this.viewport.setPerformanceMode(mode);
    this.refreshPerformance();
  }

  /** Profile counters cover the current detailed session; toggling never restarts simulation. */
  syncEffectProfiling(effect: ParticleEffect) {
    const profile = effect.profile;
    const enabled = this.performanceMode === PARTICLE_PROFILE_MODE.DETAILED;
    if(profile.enabled === enabled) {
      return;
    }
    profile.enabled = enabled;
    if(!enabled) {
      return;
    }
    profile.steps = 0;
    profile.simulationMs = 0;
    for(const row of profile.emitters) {
      row.spawnMs = 0;
      row.updateMs = 0;
      row.spawned = 0;
      row.dropped = 0;
      row.live = 0;
      row.operations = 0;
    }
    for(const particle of effect.particles) {
      if(particle.age + 1e-10 < particle.lifetime) {
        profile.emitters[particle.emitterIndex].live++;
      }
    }
  }

  refreshEmitters() {
    const items = this.asset.emitters.map((emitter, index) => node(`
      <button data-index="${index}" class="emitter-item ${index === this.selectedEmitter ? 'selected' : ''}">
        <span class="emitter-dot ${emitter.enabled ? 'on' : ''}"></span>
        <span><b>${escapeText(emitter.id)}</b><small>${emitter.root ? 'ROOT' : 'CHILD'} · ${emitter.renderer.toUpperCase()}</small></span>
      </button>
    `));
    this.elements.emitterList.replaceChildren(...items);
    get('selected-emitter').textContent = `Selected emitter: ${this.currentEmitter().id} · properties and scripts below`;
  }

  refreshParameters() {
    const entries = Object.entries(this.asset.parameters);
    if(entries.length === 0) {
      this.elements.parameterList.innerHTML = '<p class="empty">No named parameters.</p>';
      return;
    }

    this.elements.parameterList.innerHTML = entries.map(([name, item]) => parameterMarkup(name, item, this.collapsedParameters.has(name))).join('');
  }

  refreshInspector() {
    const emitter = this.currentEmitter();
    this.editors.spawn.value = emitter.spawn;
    this.editors.update.value = emitter.update;
    const texture = emitter.texture;
    const usesTexture = emitter.renderer === PARTICLE_RENDERER.TEXTURED || emitter.renderer === PARTICLE_RENDERER.FLIPBOOK;
    const references = this.asset.emitters
      .filter(item => item.id !== emitter.id)
      .map(item => `<option value="${escapeText(item.id)}">${escapeText(item.id)}</option>`)
      .join('');
    const rendererOptions = [
      PARTICLE_RENDERER.SOFT,
      PARTICLE_RENDERER.STREAK,
      PARTICLE_RENDERER.RING,
      PARTICLE_RENDERER.BILLOW,
      PARTICLE_RENDERER.CIRCLE,
      PARTICLE_RENDERER.SQUARE,
      PARTICLE_RENDERER.TEXTURED,
      PARTICLE_RENDERER.FLIPBOOK
    ]
      .map(type => `<option ${type === emitter.renderer ? 'selected' : ''}>${type}</option>`)
      .join('');
    const bursts = emitter.bursts.map((burst, index) => `
      <div>
        <input aria-label="Burst time" type="number" min="0" step="0.01" data-burst="${index}" data-field="time" value="${burst.time}"><span>s</span>
        <input aria-label="Burst count" type="number" min="1" step="1" data-numeric-integer="true" data-burst="${index}" data-field="count" value="${burst.count}">
        <button data-burst="${index}" data-field="delete">×</button>
      </div>
    `).join('');

    this.elements.emitterTab.innerHTML = `
      <div class="field-grid">
        <label>ID<input data-field="id" value="${escapeText(emitter.id)}"></label>
        <label>Renderer<select data-field="renderer">${rendererOptions}</select></label>
        <label>Duration (s)<input type="number" min="0" max="30" step="0.01" data-field="duration" value="${emitter.duration}"></label>
        <label>Rate / s<input type="number" min="0" max="2000" data-field="rate" value="${emitter.rate}"></label>
        <label>Lifetime (s)<input type="number" min="0.01" max="30" step="0.01" data-field="lifetime" value="${emitter.lifetime}"></label>
        <label class="check"><input type="checkbox" data-field="enabled" ${emitter.enabled ? 'checked' : ''}> Enabled</label>
        <label class="check"><input type="checkbox" data-field="root" ${emitter.root ? 'checked' : ''}> Root Emitter</label>
      </div>
      ${usesTexture ? `<h3>Sprite Texture</h3>
      <div class="texture-preview"><img src="${escapeText(texture?.source ?? DEFAULT_PARTICLE_TEXTURE)}" alt="Emitter sprite texture"><button data-action="upload-texture">Upload PNG</button></div>
      <p class="texture-note">PNG · up to 1 MiB · 2048 × 2048 pixels</p>
      <div class="field-grid texture-settings">
        ${['columns', 'rows', 'frames', 'fps'].map(field => `<label>${field === 'fps' ? 'Frames / s' : field}<input type="number" min="${field === 'fps' ? 0 : 1}" step="1" ${field === 'fps' ? '' : 'data-numeric-integer="true"'} data-field="texture-${field}" value="${texture?.[ ((field) as 'columns' | 'rows' | 'frames' | 'fps')] ?? 1}"></label>`).join('')}
        <label class="check"><input type="checkbox" data-field="texture-loop" ${texture?.loop ? 'checked' : ''}> Loop Animation</label>
      </div>` : ''}
      <h3>Particle Light</h3>
      <label class="check"><input type="checkbox" data-field="light-enabled" ${emitter.light ? 'checked' : ''}> Emit Light</label>
      <p class="texture-note">Each particle inherits its RGB color and alpha. Scripts can animate lightIntensity and lightRange.</p>
      ${emitter.light ? `<div class="field-grid">
        <label>Intensity<input type="number" min="0" max="1000" step="0.1" data-field="light-intensity" value="${emitter.light.intensity}"></label>
        <label>Range<input type="number" min="0.01" max="10000" step="1" data-field="light-range" value="${emitter.light.range}"></label>
      </div>` : ''}
      <h3>Bursts</h3><div class="bursts">${bursts}<button data-action="add-burst">+ Add Burst</button></div>
      <h3>Child Emission</h3>
      <div class="field-grid">
        <label>Trail Target<select data-field="trail-emitter"><option value="">None</option>${references}</select></label>
        <label>Interval<input type="number" min="0.001" step="0.01" data-field="trail-interval" value="${emitter.trail?.interval ?? 0.1}"></label>
        <label>Trail Count<input type="number" min="1" step="1" data-numeric-integer="true" data-field="trail-count" value="${emitter.trail?.count ?? 1}"></label>
        <label>Death Target<select data-field="death-emitter"><option value="">None</option>${references}</select></label>
        <label>Death Count<input type="number" min="1" step="1" data-numeric-integer="true" data-field="death-count" value="${emitter.death?.count ?? 1}"></label>
      </div>
    `;

    const trail = ((this.elements.emitterTab.querySelector('[data-field="trail-emitter"]')) as HTMLSelectElement | null);
    const death = ((this.elements.emitterTab.querySelector('[data-field="death-emitter"]')) as HTMLSelectElement | null);
    if(trail) {
      trail.value = emitter.trail?.emitter ?? '';
    }
    if(death) {
      death.value = emitter.death?.emitter ?? '';
    }
  }

  refreshTimeline() {
    const duration = this.replayWindowSeconds || this.duration;
    this.elements.timeline.max = String(duration);
    this.elements.duration.textContent = `${duration.toFixed(2)} s`;
  }

  refreshDiagnostics() {
    const effect = this.effect;
    const time = effect?.time ?? 0;
    this.elements.count.textContent = (effect?.particles.length ?? 0).toLocaleString();
    this.elements.dropped.textContent = String(effect?.droppedParticles ?? 0);
    this.elements.time.textContent = `${time.toFixed(2)} s`;
    if(!this.scrubbing) {
      this.elements.timeline.value = String(Math.min(this.duration, time));
      refreshRangeFill(this.elements.timeline);
    }
  }

  refreshPerformance() {
    if(this.performanceMode === PARTICLE_PROFILE_MODE.DISABLED) {
      return;
    }
    const stats = this.viewport.stats;
    const frameCount = Math.max(1, this.telemetryFrames);
    get('render-range').textContent = stats.range;
    get('preview-fps').textContent = this.playing ? (this.previewFps ? this.previewFps.toFixed(0) : '—') : 'Paused';
    get('simulation-ms').textContent = `${(this.simulationMilliseconds / frameCount).toFixed(2)} ms`;
    get('geometry-ms').textContent = `${stats.geometryMs.toFixed(2)} ms`;
    get('submission-ms').textContent = `${stats.submissionMs.toFixed(2)} ms`;
    get('scene-batches').textContent = String(stats.sceneBatches);
    get('triangle-count').textContent = stats.triangles.toLocaleString();
    get('light-count').textContent = `${stats.lights}${stats.omittedLights ? ` · ${stats.omittedLights} omitted (preview light cap)` : ''}`;
    const profile = this.effect?.profile;
    if(this.performanceMode === PARTICLE_PROFILE_MODE.DETAILED && profile?.enabled) {
      get('sort-ms').textContent = `${stats.sortMs.toFixed(2)} ms`;
      get('vertex-count').textContent = stats.vertices.toLocaleString();
      get('vertex-payload').textContent = `${(stats.uploadBytes/1024).toFixed(1)} KiB`;
      const reset = profile.steps < this.profileSteps;
      const steps = Math.max(1, profile.steps - (reset ? 0 : this.profileSteps));
      const rows = profile.emitters.map(emitter => {
        const previous = reset ? undefined : this.emitterSamples.get(emitter.id);
        const spawn = !this.playing && profile.steps === this.profileSteps ? previous?.spawnSample ?? 0 : Math.max(0, emitter.spawnMs - (previous?.spawnMs ?? 0)) / steps;
        const update = !this.playing && profile.steps === this.profileSteps ? previous?.updateSample ?? 0 : Math.max(0, emitter.updateMs - (previous?.updateMs ?? 0)) / steps;
        this.emitterSamples.set(emitter.id, { spawnMs: emitter.spawnMs, updateMs: emitter.updateMs, spawnSample: spawn, updateSample: update });
        const geometry = stats.byEmitter.find(item => item.id === emitter.id);
        return node(`<tr><td>${escapeText(emitter.id)}</td><td>${emitter.live}</td><td>${spawn.toFixed(3)}</td><td>${update.toFixed(3)}</td><td>${(geometry?.geometryMs ?? 0).toFixed(3)}</td><td>${geometry?.triangles ?? 0}</td><td>${geometry?.lights ?? 0}</td><td>${emitter.spawned}</td><td>${emitter.dropped}</td><td>${emitter.operations.toLocaleString()}</td></tr>`);
      });
      get('emitter-performance').replaceChildren(...rows);
      this.profileSteps = profile.steps;
    }
    this.simulationMilliseconds = 0;
    this.telemetryFrames = 0;
  }

  /** @param event */
  clickEmitterInspector(event: Event) {
    const target = ((event.target) as HTMLElement);
    if(target.dataset.action === 'upload-texture') {
      this.elements.textureFile.click();
      return;
    }
    if((target.dataset.action === 'add-burst' || target.dataset.field === 'delete') && !this.numeric.validate(false)) {
      return;
    }
    if(target.dataset.action === 'add-burst') {
      this.currentEmitter().bursts.push({ time: 0, count: 10 });
      this.refreshInspector();
      this.documentChanged();
    } else if(target.dataset.field === 'delete') {
      this.editEmitter(event);
    }
  }

  /** @param event */
  editEmitter(event: Event) {
    this.editEmitterInput(((event.target) as HTMLInputElement | HTMLSelectElement));
  }

  /** @param target */
  editEmitterInput(target: HTMLInputElement | HTMLSelectElement) {
    const field = target.dataset.field;
    if(!field) {
      return;
    }

    if(target instanceof HTMLInputElement && target.type === 'number' && (!Number.isFinite(target.valueAsNumber) || (target.dataset.numericInteger === 'true' && !Number.isInteger(target.valueAsNumber)) || target.validity.rangeUnderflow || target.validity.rangeOverflow || target.validity.customError)) {
      target.setCustomValidity('Enter a finite number within the displayed bounds.');
      this.numeric.validate(false);
      return;
    }
    if(!(target instanceof HTMLInputElement && target.type === 'number') && !this.numeric.validate(false)) {
      return;
    }
    const emitter = this.currentEmitter();
    const burstIndex = target.dataset.burst;
    if(burstIndex !== undefined) {
      if(field === 'delete') {
        emitter.bursts.splice(Number(burstIndex), 1);
      } else if(target instanceof HTMLInputElement) {
        if(field === 'time') {
          emitter.bursts[Number(burstIndex)].time = target.valueAsNumber;
        }
        if(field === 'count') {
          emitter.bursts[Number(burstIndex)].count = target.valueAsNumber;
        }
      }
      if(field === 'delete') {
        this.refreshInspector();
      }
      this.documentChanged();
      return;
    }

    if(field === 'light-enabled') {
      if(((target) as HTMLInputElement).checked) {
        emitter.light = { intensity: 2, range: 220 };
      } else {
        delete emitter.light;
      }
      this.refreshInspector();
    } else if(field === 'light-intensity' && emitter.light && target instanceof HTMLInputElement) {
      emitter.light.intensity = target.valueAsNumber;
    } else if(field === 'light-range' && emitter.light && target instanceof HTMLInputElement) {
      emitter.light.range = target.valueAsNumber;
    } else if(field === 'enabled') {
      emitter.enabled = ((target) as HTMLInputElement).checked;
    } else if(field === 'root') {
      emitter.root = ((target) as HTMLInputElement).checked;
    } else if(target instanceof HTMLInputElement && field === 'duration') {
      emitter.duration = target.valueAsNumber;
    } else if(target instanceof HTMLInputElement && field === 'rate') {
      emitter.rate = target.valueAsNumber;
    } else if(target instanceof HTMLInputElement && field === 'lifetime') {
      emitter.lifetime = target.valueAsNumber;
    } else if(field === 'id') {
      this.renameEmitter(emitter, target.value);
    } else if(field === 'renderer') {
      emitter.renderer = ((target.value) as ParticleEmitterAsset['renderer']);
      if((emitter.renderer === PARTICLE_RENDERER.TEXTURED || emitter.renderer === PARTICLE_RENDERER.FLIPBOOK) && !emitter.texture) {
        emitter.texture = { source: DEFAULT_PARTICLE_TEXTURE, columns: 1, rows: 1, frames: 1, fps: 12, loop: true };
      }
      this.refreshInspector();
    } else if(field.startsWith('texture-') && emitter.texture && target instanceof HTMLInputElement) {
      const textureField = field.slice(8);
      if(textureField === 'loop') {
        emitter.texture.loop = target.checked;
      } else if(textureField === 'columns' || textureField === 'rows' || textureField === 'frames' || textureField === 'fps') {
        emitter.texture[textureField] = target.valueAsNumber;
      }
    } else if(field.startsWith('trail-')) {
      setChildEmitter(emitter, 'trail', field.slice(6), target);
    } else if(field.startsWith('death-')) {
      setChildEmitter(emitter, 'death', field.slice(6), target);
    }
    this.refreshEmitters();
    this.documentChanged();
  }

  /** @param emitter @param nextId */
  renameEmitter(emitter: ParticleEmitterAsset, nextId: string) {
    const previousId = emitter.id;
    if(nextId !== previousId && this.asset.emitters.some(item => item.id === nextId)) {
      this.showError(`Emitter '${nextId}' already exists.`);
      this.refreshInspector();
      return;
    }
    emitter.id = nextId;
    for(const item of this.asset.emitters) {
      if(item.trail?.emitter === previousId) {
        item.trail.emitter = nextId;
      }
      if(item.death?.emitter === previousId) {
        item.death.emitter = nextId;
      }
    }
  }

  /** @param event */
  editParameter(event: Event) {
    this.editParameterInput(((event.target) as HTMLInputElement | HTMLButtonElement | HTMLSelectElement), event.type);
  }

  /** @param target @param eventType */
  editParameterInput(target: HTMLInputElement | HTMLButtonElement | HTMLSelectElement, eventType: string) {
    const name = target.dataset.param;
    const field = target.dataset.field;
    if(!name || !field) {
      return;
    }

    if(field === 'collapse' && eventType === 'click') {
      const card = target.closest('.parameter');
      if(!(card instanceof HTMLElement)) {
        return;
      }
      const collapsed = !this.collapsedParameters.has(name);
      if(collapsed) {
        this.collapsedParameters.add(name);
      } else {
        this.collapsedParameters.delete(name);
      }
      setParameterCollapsed(card, name, this.asset.parameters[name], collapsed);
      return;
    }

    // Commit text/numeric edits on change; ranges respond continuously while dragging.
    if(eventType === 'click' && field !== 'delete') {
      return;
    }
    if(eventType === 'input' && !(target instanceof HTMLInputElement && target.type === 'range')) {
      return;
    }
    if(eventType === 'change' && target instanceof HTMLInputElement && target.type === 'range') {
      return;
    }

    if(field === 'delete') {
      if(this.parameterIsUsed(name)) {
        this.showError(`Parameter '${name}' is used by a script and cannot be deleted.`);
        return;
      }
      delete this.asset.parameters[name];
      this.collapsedParameters.delete(name);
      this.refreshParameters();
      this.documentChanged();
      return;
    }

    const item = this.asset.parameters[name];
    if(field === 'name' && target instanceof HTMLInputElement) {
      const nextName = target.value.trim();
      if(!nextName || nextName === name) {
        return;
      }
      if(this.asset.parameters[nextName]) {
        this.showError(`Parameter '${nextName}' already exists.`);
        this.refreshParameters();
        return;
      }
      if(this.parameterIsUsed(name)) {
        this.showError(`Parameter '${name}' is used by a script and cannot be renamed.`);
        this.refreshParameters();
        return;
      }
      delete this.asset.parameters[name];
      this.asset.parameters[nextName] = item;
      if(this.collapsedParameters.delete(name)) {
        this.collapsedParameters.add(nextName);
      }
      this.refreshParameters();
    } else if(field === 'type' && target instanceof HTMLSelectElement) {
      if(this.parameterIsUsed(name)) {
        this.showError(`Parameter '${name}' is used by a script; remove its references before changing type.`);
        this.refreshParameters();
        return;
      }
      if(target.value === 'vector3') {
        this.asset.parameters[name] = { type: PARTICLE_PARAMETER_TYPE.VECTOR3, value: [1, 1, 1], min: [0, 0, 0], max: [2, 2, 2] };
      } else if(target.value === 'color') {
        this.asset.parameters[name] = { type: PARTICLE_PARAMETER_TYPE.COLOR, value: [1, 1, 1, 1], min: [0, 0, 0, 0], max: [1, 1, 1, 1] };
      } else {
        this.asset.parameters[name] = { value: 1, min: 0, max: 2 };
      }
      this.refreshParameters();
    } else if(target instanceof HTMLInputElement && (field === 'value' || field === 'min' || field === 'max')) {
      const component = Number(target.dataset.component);
      const value = target.valueAsNumber;
      const minimum = Array.isArray(item.min) ? item.min[component] : item.min;
      const maximum = Array.isArray(item.max) ? item.max[component] : item.max;
      const lower = field === 'value' ? minimum : item.type === PARTICLE_PARAMETER_TYPE.COLOR ? 0 : -1e6;
      const upper = field === 'value' ? maximum : item.type === PARTICLE_PARAMETER_TYPE.COLOR ? 1 : 1e6;
      if(!Number.isFinite(value) || value < lower || value > upper || (field === 'min' && value > maximum) || (field === 'max' && value < minimum)) {
        target.setCustomValidity(`Enter a finite ${field} within ${lower}–${upper}, with minimum no greater than maximum.`);
        this.numeric.validate(false);
        return;
      }
      const current = item[field];
      if(Array.isArray(current)) {
        current[component] = value;
      } else if(item.type !== PARTICLE_PARAMETER_TYPE.VECTOR3 && item.type !== PARTICLE_PARAMETER_TYPE.COLOR) {
        item[field] = value;
      }
      // Tightened bounds clamp the value and immediately update both value controls.
      const nextMinimum = Array.isArray(item.min) ? item.min[component] : item.min;
      const nextMaximum = Array.isArray(item.max) ? item.max[component] : item.max;
      const previousValue = Array.isArray(item.value) ? item.value[component] : item.value;
      const nextValue = Math.max(nextMinimum, Math.min(nextMaximum, previousValue));
      if(Array.isArray(item.value)) {
        item.value[component] = nextValue;
      } else {
        item.value = nextValue;
      }
      const card = target.closest('.parameter');
      for(const input of card?.querySelectorAll(`input[data-component="${component}"][data-field="value"]`) ?? []) {
        if(input instanceof HTMLInputElement) {
          input.min = String(nextMinimum);
          input.max = String(nextMaximum);
          input.setCustomValidity('');
          input.value = String(nextValue);
          if(input.type === 'range') {
            refreshRangeFill(input);
          }
        }
      }
      const swatch = card?.querySelector('.color-swatch i');
      if(swatch instanceof HTMLElement && item.type === PARTICLE_PARAMETER_TYPE.COLOR) {
        swatch.style.background = colorStyle(item.value);
      }
    }
    this.documentChanged();
  }

  /** @param name */
  parameterIsUsed(name: string) {
    const parameter = this.asset.parameters[name];
    const scriptNames = parameter?.type === PARTICLE_PARAMETER_TYPE.VECTOR3 ? [name, `${name}X`, `${name}Y`, `${name}Z`] : [name];
    return scriptNames.some(scriptName => {
      const pattern = new RegExp(`(^|[^A-Za-z0-9_])${escapeRegularExpression(scriptName)}([^A-Za-z0-9_]|$)`);
      return this.asset.emitters.some(emitter => pattern.test(emitter.spawn) || pattern.test(emitter.update));
    });
  }

  /** @param preset */
  addEmitter(preset: string) {
    if(!this.numeric.validate(false)) {
      return;
    }
    this.numeric.clear();
    this.asset.emitters.push(createEmitterTemplate(uniqueEmitterId(this.asset, 'emitter'), preset));
    this.selectedEmitter = this.asset.emitters.length - 1;
    this.refreshAll();
    this.documentChanged();
  }

  duplicateEmitter() {
    if(!this.numeric.validate(false)) {
      return;
    }
    this.numeric.finish();
    const copy = structuredClone(this.currentEmitter());
    copy.id = uniqueEmitterId(this.asset, `${copy.id}_copy`);
    copy.root = true;
    this.asset.emitters.push(copy);
    this.selectedEmitter = this.asset.emitters.length - 1;
    this.refreshAll();
    this.documentChanged();
  }

  deleteEmitter() {
    if(!this.numeric.validate(false)) {
      return;
    }
    this.numeric.finish();
    if(this.asset.emitters.length <= 1) {
      this.showError('An asset needs at least one emitter.');
      return;
    }
    const removedId = this.currentEmitter().id;
    this.asset.emitters.splice(this.selectedEmitter, 1);
    this.selectedEmitter = Math.max(0, this.selectedEmitter - 1);
    for(const emitter of this.asset.emitters) {
      if(emitter.trail?.emitter === removedId) {
        delete emitter.trail;
      }
      if(emitter.death?.emitter === removedId) {
        delete emitter.death;
      }
    }
    this.refreshAll();
    this.documentChanged();
  }

  addParameter() {
    if(!this.numeric.validate(false)) {
      return;
    }
    this.numeric.finish();
    let index = 1;
    while(this.asset.parameters[`parameter${index}`]) {
      index += 1;
    }
    this.asset.parameters[`parameter${index}`] = { value: 1, min: 0, max: 2 };
    this.refreshParameters();
    this.documentChanged();
  }

  /** @param tab */
  selectTab(tab: string) {
    for(const button of document.querySelectorAll('.tabs button')) {
      const isSelected = ((button) as HTMLButtonElement).dataset.tab === tab;
      button.setAttribute('aria-pressed', String(isSelected));
    }
    for(const page of document.querySelectorAll('.tab-page')) {
       ((page) as HTMLElement).hidden = page.id !== `${tab}-tab`;
    }
  }

  async openFile() {
    const file = this.elements.file.files?.[0];
    if(!file) {
      return;
    }
    if(file.size > MAX_ASSET_BYTES) {
      this.showError(`Particle assets cannot exceed ${MAX_ASSET_BYTES.toLocaleString()} bytes.`);
      this.elements.file.value = '';
      return;
    }

    try {
      const candidate = readAsset(await file.text());
      this.asset = candidate;
      this.collapsedParameters.clear();
      this.selectedEmitter = 0;
      this.refreshAll();
      this.apply();
    } catch(error) {
      this.showError(error instanceof Error ? error.message : String(error));
    } finally {
      this.elements.file.value = '';
    }
  }

  async uploadTexture() {
    const file = this.elements.textureFile.files?.[0];
    this.elements.textureFile.value = '';
    if(!file) {
      return;
    }
    const asset = this.asset;
    const emitter = this.currentEmitter();
    const generation = ++this.textureUploadGeneration;
    try {
      if(file.size > 1024 * 1024 || file.type !== 'image/png') {
        throw new Error('Choose a PNG texture up to 1 MiB.');
      }
      const bitmap = await createImageBitmap(file);
      const validSize = bitmap.width <= 2048 && bitmap.height <= 2048;
      bitmap.close();
      if(!validSize) {
        throw new Error('Texture dimensions cannot exceed 2048 × 2048 pixels.');
      }
      const source = await readDataUrl(file);
      if(this.disposed || generation !== this.textureUploadGeneration || this.asset !== asset || this.currentEmitter() !== emitter) {
        return;
      }
      emitter.texture = { source, columns: 1, rows: 1, frames: 1, fps: 12, loop: true };
      this.refreshInspector();
      this.documentChanged();
    } catch(error) {
      if(!this.disposed && this.asset === asset && this.currentEmitter() === emitter) {
        this.showError(error instanceof Error ? error.message : String(error));
      }
    }
  }

  populateExamples() {
    const options = EXAMPLES.map((example, index) => node(
      `<option value="${index}">${escapeText(example.label)}</option>`,
    ));
    this.elements.examples.replaceChildren(...options);
  }

  currentEmitter() {
    return this.asset.emitters[Math.min(this.selectedEmitter, this.asset.emitters.length - 1)];
  }

  /** @param message */
  showError(message: string) {
    this.elements.error.hidden = false;
    this.elements.error.textContent = message;
    this.elements.diagnostic.textContent = 'LIVE · FIX ERROR';
  }

  hideError() {
    this.elements.error.hidden = true;
    this.elements.error.textContent = '';
  }

  /** @param message */
  showGpuError(message: string) {
    this.elements.gpuError.hidden = false;
    this.elements.gpuError.textContent = `GPU preview unavailable: ${message}`;
    this.elements.backend.textContent = 'GPU ERROR';
  }

  destroy() {
    if(this.disposed) {
      return;
    }
    this.disposed = true;
    this.applyGeneration += 1;
    this.seekGeneration += 1;
    clearTimeout(this.applyTimer);
    cancelAnimationFrame(this.frameRequest);
    this.panelLayout?.destroy();
    this.listeners.abort();
    this.effect?.destroy();
    this.viewport.destroy();
    for(const editor of Object.values(this.editors)) {
      editor.destroy();
    }
  }
}

function resolveElements() {
  return {
    canvas: get('viewport', HTMLCanvasElement),
    examples: get('examples', HTMLSelectElement),
    emitterList: get('emitter-list'),
    parameterList: get('parameter-list'),
    emitterTab: get('emitter-tab'),
    spawn: get('spawn-source'),
    update: get('update-source'),
    name: get('asset-name', HTMLInputElement),
    timeline: get('timeline', HTMLInputElement),
    time: get('time'),
    duration: get('duration'),
    count: get('particle-count'),
    dropped: get('dropped-count'),
    backend: get('backend'),
    diagnostic: get('diagnostic'),
    error: get('error'),
    gpuError: get('gpu-error'),
    file: get('file-input', HTMLInputElement),
    textureFile: get('texture-input', HTMLInputElement),
    live: get('live', HTMLInputElement),
    loop: get('loop', HTMLInputElement),
    play: get('play', HTMLButtonElement),
    seed: get('seed', HTMLInputElement),
    speed: get('speed', HTMLSelectElement),
  };
}

/** Require a matching element during startup, preserving the requested DOM subtype. */
function get(id: string): HTMLElement;
function get<T extends HTMLElement>(id: string, kind: new () => T): T;
function get(id: string, kind: typeof HTMLElement = HTMLElement) {
  const element = document.getElementById(id);
  if(!(element instanceof kind)) {
    throw new Error(`Missing #${id}.`);
  }
  return element;
}

/** @param html */
function node(html: string) {
  const template = document.createElement('template');
  template.innerHTML = html.trim();
  return  ((template.content.firstElementChild) as HTMLElement);
}

/** @param value */
function escapeText(value: string) {
  return value.replace(/[&<>"']/g, character => {
    if(character === '&') {
      return '&amp;';
    }
    if(character === '<') {
      return '&lt;';
    }
    if(character === '>') {
      return '&gt;';
    }
    if(character === '"') {
      return '&quot;';
    }
    return '&#39;';
  });
}

function loadInitialAsset() {
  try {
    const saved = hosted ? null : readStoredText(STORAGE_KEY);
    return saved ? readAsset(saved) : copyExample(0);
  } catch {
    return copyExample(0);
  }
}

/** @param asset */
function calculateDuration(asset: ParticleEffectAsset) {
  // Child chains and scripts can extend well beyond a root emitter's own timing.
  // The format's hard seek horizon exposes every valid effect without guessing.
  void asset;
  return 30;
}

/** @param asset @param base */
function uniqueEmitterId(asset: ParticleEffectAsset, base: string) {
  let id = base.replace(/[^A-Za-z0-9_]/g, '_');
  let index = 2;
  while(asset.emitters.some(item => item.id === id)) {
    id = `${base}_${index}`.replace(/[^A-Za-z0-9_]/g, '_');
    index += 1;
  }
  return id;
}

/** @param target */
function isEditing(target: EventTarget | null) {
  return target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLSelectElement ||
    (target instanceof HTMLElement && (target.isContentEditable || Boolean(target.closest('.cm-editor'))));
}

/**
 * @param emitter
 * @param type
 * @param field
 * @param target
 */
function setChildEmitter(emitter: ParticleEmitterAsset, type: 'trail' | 'death', field: string, target: HTMLInputElement | HTMLSelectElement) {
  if(field === 'emitter') {
    if(!target.value) {
      if(type === 'trail') {
        delete emitter.trail;
      } else {
        delete emitter.death;
      }
      return;
    }
    if(type === 'trail') {
      emitter.trail = { emitter: target.value, interval: 0.1, count: 1 };
    } else {
      emitter.death = { emitter: target.value, count: 1 };
    }
    return;
  }

  if(!(target instanceof HTMLInputElement)) {
    return;
  }
  if(type === 'trail' && emitter.trail) {
    if(field === 'interval') {
      emitter.trail.interval = target.valueAsNumber;
    } else if(field === 'count') {
      emitter.trail.count = target.valueAsNumber;
    }
  }
  if(type === 'death' && emitter.death && field === 'count') {
    emitter.death.count = target.valueAsNumber;
  }
}

/** @param value */
function escapeRegularExpression(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/** @param file */
function readDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(new Error('Could not read texture PNG.'));
    reader.readAsDataURL(file);
  });
}

function nextAnimationFrame() {
  return new Promise(resolve => requestAnimationFrame(resolve));
}

const application = new ParticleLab();
const navigation = mountEditorNavigation({
  currentTool: 'particle-lab',
  openButton: get('open', HTMLButtonElement),
  saveButton: get('save', HTMLButtonElement)
});
const workspaceBridge = connectWorkspaceTool({
  deferFocusedInputs: true,
  maximized: value => application.panelLayout.setMaximized(value),
  load(path: string, text: string) {
    const asset = readAsset(text);
    application.numeric.clear();
    application.asset = asset;
    application.collapsedParameters.clear();
    application.selectedEmitter = 0;
    application.refreshAll();
    void application.apply();
  },
  serialize() {
    application.numeric.capture();
    return writeAsset(application.asset);
  }
});
application.initialize();
window.addEventListener(BROWSER_EVENT.PAGEHIDE, event => {
  if(!event.persisted) {
    workspaceBridge.destroy();
    navigation.destroy();
    application.destroy();
  }
});
if(import.meta.hot) {
  import.meta.hot.dispose(() => {
    workspaceBridge.destroy();
    navigation.destroy();
    application.destroy();
  });
}
