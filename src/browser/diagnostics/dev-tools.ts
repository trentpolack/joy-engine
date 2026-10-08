// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT } from '../constants.ts';
import { KEY_CODE } from '../input/constants.ts';
import styles from './dev-tools.css?raw';
import themeStyles from '../dev-widget-theme.css?raw';
import launcherStyles from '../editor-launchers.css?raw';
import { EDITOR_PANEL_OPEN_EVENT, announceEditorPanelOpen, isAnotherEditorOpening } from '../editor-launchers.ts';
import type { RendererDiagnosticSource } from '../../rendering/gpu/gpu-diagnostics.ts';
import { requireElement } from '../dom.ts';

type DetailGroup = 'runtime' | 'timing' | 'heap' | 'gpu-memory' | 'gpu-resources';
const RESOURCE_LABELS = [
  ['Resources', 'resources'],
  ['Buffers', 'buffers'],
  ['Textures', 'textures'],
  ['RenderPipelines', 'render pipelines'],
  ['ComputePipelines', 'compute pipelines'],
  ['Framebuffers', 'framebuffers']
] as const;

export interface DevToolsOptions {
  title?: string;
  enabled?: boolean;
  sampleWindowMilliseconds?: number;
}

/**
 * Own the development summary, detail panel, and their event listeners.
 * Games supply runtime metadata and one animation-frame timestamp per render.
 * A disabled instance creates no DOM or listeners and emits no logs. The owner
 * must call destroy() on disposal; destroying one instance leaves others intact.
 */
export class DevTools {
  declare enabled: boolean;
  declare sampleWindowMilliseconds: number;
  declare frameCount: number;
  declare sampleStartedAt: number | null;
  declare view: ReturnType<typeof createView> | null;
  declare rows: Map<string, HTMLElement>;
  private declare values: Map<string, {text: string; group: DetailGroup}>;
  private declare renderer: RendererDiagnosticSource | null;
  private declare onVisibilityChange: () => void;
  declare onKeyDown: (event: KeyboardEvent) => void;
  declare onClick: () => void;
  private declare onEditorPanelOpen: (event: Event) => void;
  private declare onPanelKeyDown: (event: KeyboardEvent) => void;

  /**
   * @param [options] Runtime identity and sampling configuration.
   */
  constructor({ title = 'JOY DEV', enabled = true, sampleWindowMilliseconds = 500 }: DevToolsOptions = {}) {
    this.enabled = enabled;
    this.sampleWindowMilliseconds = sampleWindowMilliseconds;

    // Sample state uses the caller's clock, never construction or simulation time.
    this.frameCount = 0;
    this.sampleStartedAt = null;

    // This owner retains its DOM, styles, and handler identities for cleanup.
    this.view = null;
    this.rows = new Map();
    this.values = new Map();
    this.renderer = null;
    this.onVisibilityChange = this.resetFrameSample.bind(this);
    this.onKeyDown = this.handleKeyDown.bind(this);
    this.onClick = this.togglePanel.bind(this);
    this.onEditorPanelOpen = this.handleEditorPanelOpen.bind(this);
    this.onPanelKeyDown = this.handlePanelKeyDown.bind(this);

    if(!enabled) {
      return;
    }
    if(!Number.isFinite(sampleWindowMilliseconds) || sampleWindowMilliseconds <= 0) {
      throw new RangeError('Development sample window must be a positive number of milliseconds.');
    }

    this.view = createView(title);
    this.view.summary.addEventListener(BROWSER_EVENT.CLICK, this.onClick);
    document.addEventListener(BROWSER_EVENT.KEYDOWN, this.onKeyDown);
    document.addEventListener(BROWSER_EVENT.VISIBILITYCHANGE, this.onVisibilityChange);
    document.addEventListener(EDITOR_PANEL_OPEN_EVENT, this.onEditorPanelOpen);
    this.view.root.addEventListener(BROWSER_EVENT.KEYDOWN, this.onPanelKeyDown);
    this.set('mode', 'development');
    this.set('viewport', `${window.innerWidth} × ${window.innerHeight} CSS px`);
    this.updateMemoryStats();
    this.setDetail('CPU time', 'Not collected', 'timing');
    this.setDetail('GPU time', 'Not collected', 'timing');
    this.log('Runtime diagnostics enabled. Press ` to toggle details.');
  }

  /**
   * Sample the interval between rendered frames, including time spent waiting
   * for the next frame. This is frame cadence, not CPU work or GPU execution time.
   * @param timestamp Monotonic requestAnimationFrame time in milliseconds.
   */
  frame(timestamp: number) {
    if(!this.enabled || !this.view) {
      return;
    }
    if(document.hidden || !Number.isFinite(timestamp)) {
      this.resetFrameSample();
      return;
    }
    if(this.sampleStartedAt === null || timestamp <= this.sampleStartedAt) {
      this.sampleStartedAt = timestamp;
      this.frameCount = 0;
      return;
    }

    this.frameCount+= 1;
    const elapsedMilliseconds = timestamp - this.sampleStartedAt;
    if(elapsedMilliseconds < this.sampleWindowMilliseconds) {
      return;
    }

    const framesPerSecond = this.frameCount*1000/elapsedMilliseconds;
    const frameMilliseconds = elapsedMilliseconds/this.frameCount;
    this.view.fpsOutput.textContent = `${Math.round(framesPerSecond)} FPS`;
    this.view.frameOutput.textContent = `${frameMilliseconds.toFixed(1)} MS`;
    this.setDetail('FPS', Math.round(framesPerSecond), 'timing');
    this.setDetail('frame interval', `${frameMilliseconds.toFixed(1)} ms`, 'timing');
    this.updateMemoryStats();
    this.updateRendererStats();
    this.flushDetails();
    this.frameCount = 0;
    this.sampleStartedAt = timestamp;
  }

  /**
   * Add or replace a display value without retaining the caller's mutable data.
   * @param name Stable detail label, such as renderer or viewport.
   * @param value Current value, copied as text.
   */
  set(name: string, value: string | number) {
    if(!this.enabled || !this.view) {
      return;
    }
    this.setDetail(name, value, 'runtime');
  }

  /**
   * Borrow an optional renderer for on-demand detail reads; no subscriptions are added.
   * Disabled instances do not retain it. Pass null to detach before replacing its owner.
   */
  setRenderer(renderer: RendererDiagnosticSource | null) {
    if(!this.enabled || !this.view) {
      return;
    }
    this.renderer = renderer;
    // Do not show a previous owner's measurements while awaiting the next sample.
    for(const [name, value] of this.values) {
      if(value.group === 'gpu-memory' || value.group === 'gpu-resources') {
        this.values.delete(name);
        const output = this.rows.get(name);
        output?.previousElementSibling?.remove();
        output?.remove();
        this.rows.delete(name);
      }
    }
  }

  /**
   * Emit an intentional startup or lifecycle message in the browser console.
   * @param message Human-readable event description.
   * @param [details] Optional browser-inspectable diagnostic data.
   */
  log(message: string, details?: unknown) {
    if(!this.enabled) {
      return;
    }
    if(details === undefined) {
      console.info(`[Joy Dev] ${message}`);
      return;
    }
    console.info(`[Joy Dev] ${message}`, details);
  }

  /** Toggle runtime details through the same path for pointer and keyboard input. */
  togglePanel() {
    if(!this.enabled || !this.view) {
      return;
    }
    this.setPanelOpen(Boolean(this.view.panel.hidden));
  }

  /** Opening a tool closes its siblings without discarding their authoring state. */
  private setPanelOpen(expanded: boolean) {
    if(!this.view) {
      return;
    }
    this.view.panel.hidden = !expanded;
    this.view.summary.setAttribute('aria-expanded', String(expanded));
    if(expanded) {
      announceEditorPanelOpen(this.view.root);
      this.flushDetails();
    }
  }

  /** Release owned DOM, styles, and listeners. Repeated calls are harmless. */
  destroy() {
    if(!this.enabled) {
      return;
    }
    document.removeEventListener(BROWSER_EVENT.KEYDOWN, this.onKeyDown);
    document.removeEventListener(BROWSER_EVENT.VISIBILITYCHANGE, this.onVisibilityChange);
    document.removeEventListener(EDITOR_PANEL_OPEN_EVENT, this.onEditorPanelOpen);
    if(this.view) {
      this.view.summary.removeEventListener(BROWSER_EVENT.CLICK, this.onClick);
      this.view.root.removeEventListener(BROWSER_EVENT.KEYDOWN, this.onPanelKeyDown);
      this.view.root.remove();
      this.view.style.remove();
      this.view = null;
    }
    this.rows.clear();
    this.values.clear();
    this.renderer = null;
    this.enabled = false;
  }

  /** Start a fresh cadence window after hiding/resuming; background delay is not a slow frame. */
  private resetFrameSample() {
    this.frameCount = 0;
    this.sampleStartedAt = null;
  }

  /** Retain only the latest text; frequent game metadata updates do not write DOM. */
  private setDetail(name: string, value: string | number, group: DetailGroup) {
    this.values.set(name, {text: String(value), group});
  }

  /** Refresh visible detail rows at the sampling cadence, skipping unchanged text. */
  private flushDetails() {
    if(!this.view || this.view.panel.hidden) {
      return;
    }
    for(const [name, value] of this.values) {
      const group = this.view.groups[value.group];
      if(!group.details.open) {
        continue;
      }
      let output = this.rows.get(name);
      if(!output) {
        const term = document.createElement('dt');
        term.textContent = name;
        output = document.createElement('dd');
        group.list.append(term, output);
        this.rows.set(name, output);
      }
      if(output.textContent !== value.text) {
        output.textContent = value.text;
      }
    }
  }

  /** Read only requested sections; these luma counters aggregate all devices in this realm. */
  private updateRendererStats() {
    if(!this.view || this.view.panel.hidden) {
      return;
    }
    const memory = this.view.groups['gpu-memory'].details.open;
    const resources = this.view.groups['gpu-resources'].details.open;
    if(!memory && !resources) {
      return;
    }
    const snapshot = this.renderer?.getDiagnostics({memory, resources});
    if(memory) {
      this.setDetail('tracked allocations', formatOptionalBytes(snapshot?.memory?.allocatedBytes), 'gpu-memory');
      this.setDetail('buffer allocations', formatOptionalBytes(snapshot?.memory?.bufferBytes), 'gpu-memory');
      this.setDetail('texture allocations', formatOptionalBytes(snapshot?.memory?.textureBytes), 'gpu-memory');
    }
    if(resources) {
      const counters = snapshot?.resources;
      for(const [name, label] of RESOURCE_LABELS) {
        const value = counters?.[name];
        this.setDetail(`${label} active / created`, `${value?.active ?? 'Unavailable'} / ${value?.created ?? 'Unavailable'}`, 'gpu-resources');
      }
      if(snapshot?.scene) {
        this.setDetail('this scene visible / culled', `${snapshot.scene.visible} / ${snapshot.scene.culled}`, 'gpu-resources');
      } else if(this.values.has('this scene visible / culled')) {
        this.setDetail('this scene visible / culled', 'Unavailable', 'gpu-resources');
      }
    }
  }

  /**
   * Toggle details for the shared diagnostic shortcut.
   * @private
   * @param event Borrowed browser event.
   */
  private handleKeyDown(event: KeyboardEvent) {
    if(event.key === KEY_CODE.ESCAPE && this.view && !this.view.panel.hidden) {
      event.preventDefault();
      event.stopPropagation();
      this.setPanelOpen(false);
      this.view.summary.focus();
      return;
    }
    if(event.code !== 'Backquote' || event.repeat || event.ctrlKey || event.metaKey || event.altKey) {
      return;
    }
    const target = event.target;
    if(target instanceof HTMLElement && target.closest('input, textarea, select, [contenteditable]')) {
      return;
    }
    event.preventDefault();
    this.togglePanel();
  }

  /** React only to another owner's announcement; closed tools retain their state. */
  private handleEditorPanelOpen(event: Event) {
    if(this.view && isAnotherEditorOpening(event, this.view.root)) {
      this.setPanelOpen(false);
    }
  }

  /** Keep development controls out of gameplay; Escape returns focus to its launcher. */
  private handlePanelKeyDown(event: KeyboardEvent) {
    event.stopPropagation();
    if(event.code === 'Backquote') {
      this.handleKeyDown(event);
      return;
    }
    if(event.key === KEY_CODE.ESCAPE && this.view) {
      event.preventDefault();
      this.setPanelOpen(false);
      this.view.summary.focus();
    }
  }

  /**
   * Refresh optional heap counters without implying total application memory.
   * @private
   */
  private updateMemoryStats() {
    if(!this.view) {
      return;
    }
    const memory = (performance as Performance & {
      memory?: {
        usedJSHeapSize: number;
        totalJSHeapSize: number;
        jsHeapSizeLimit: number;
      };
    }).memory;
    if(!memory) {
      this.view.memoryOutput.textContent = 'HEAP N/A';
      this.setDetail('JS heap', 'Not exposed by this browser', 'heap');
      this.view.memoryNote.textContent = 'This browser does not expose JavaScript heap counters. Use its developer tools to inspect memory.';
      return;
    }

    this.view.memoryOutput.textContent = `HEAP ${formatMemoryBytes(memory.usedJSHeapSize)}`;
    this.setDetail('JS heap used', formatMemoryBytes(memory.usedJSHeapSize), 'heap');
    this.setDetail('JS heap allocated', formatMemoryBytes(memory.totalJSHeapSize), 'heap');
    this.setDetail('JS heap limit', formatMemoryBytes(memory.jsHeapSizeLimit), 'heap');
    this.view.memoryNote.textContent = 'Approximate JavaScript heap reported by the browser. Excludes GPU allocations and is not total game memory.';
  }
}

/**
 * Build one instance's view; caller owns every returned element.
 * @param title
 */
function createView(title: string) {
  const root = document.createElement('aside');
  root.className = 'joy-dev-tools';
  root.setAttribute('aria-label', 'Development tools');
  root.innerHTML = `
    <button type="button" class="joy-dev-summary" aria-label="Toggle development details" aria-expanded="false">
      <b>STATS</b>
      <span data-stat="fps">-- FPS</span>
    </button>
    <section class="joy-dev-panel" hidden>
      <header><b data-stat="title"></b> <kbd title="Press the backquote key to toggle runtime details">Backquote</kbd></header>
      <p class="joy-dev-readout">
        <span data-stat="frame" title="Frame interval, including waits; not CPU or GPU execution time">-- MS</span>
        <span data-stat="memory" title="Approximate JavaScript heap usage; open details for browser support and scope">HEAP N/A</span>
      </p>
      <details class="joy-dev-group" data-group="runtime" open>
        <summary>Runtime</summary><dl></dl>
      </details>
      <details class="joy-dev-group" data-group="timing" open>
        <summary>Frame cadence</summary><dl></dl>
        <p>Frame interval includes waiting for the next frame. CPU and GPU execution timing is not collected.</p>
      </details>
      <details class="joy-dev-group" data-group="heap">
        <summary>JavaScript heap</summary><dl></dl>
        <p class="joy-dev-memory-note"></p>
      </details>
      <details class="joy-dev-group" data-group="gpu-memory">
        <summary>GPU allocations · all luma devices</summary><dl></dl>
        <p>Luma-tracked allocation bytes across all luma devices in this JavaScript realm. Estimates exclude untracked browser and driver memory; this is not total GPU memory.</p>
      </details>
      <details class="joy-dev-group" data-group="gpu-resources">
        <summary>GPU resources · all luma devices</summary><dl></dl>
        <p>Active handles and lifetime-created handles across all luma devices in this JavaScript realm. These are not draw calls.</p>
      </details>
    </section>
  `;
  requireElement(root, '[data-stat="title"]', HTMLElement).textContent = title;

  const style = document.createElement('style');
  style.textContent = `${themeStyles}\n${styles}\n${launcherStyles}`;
  document.head.append(style);
  document.body.append(root);

  return {
    root,
    style,
    summary: requireElement(root, 'button', HTMLButtonElement),
    panel: requireElement(root, '.joy-dev-panel', HTMLElement),
    groups: {
      runtime: findGroup(root, 'runtime'),
      timing: findGroup(root, 'timing'),
      heap: findGroup(root, 'heap'),
      'gpu-memory': findGroup(root, 'gpu-memory'),
      'gpu-resources': findGroup(root, 'gpu-resources')
    },
    fpsOutput: requireElement(root, '[data-stat="fps"]', HTMLElement),
    frameOutput: requireElement(root, '[data-stat="frame"]', HTMLElement),
    memoryOutput: requireElement(root, '[data-stat="memory"]', HTMLElement),
    memoryNote: requireElement(root, '.joy-dev-memory-note', HTMLElement)
  };
}

/**
 * Format browser-reported heap bytes as human-readable mebibytes.
 * @param bytes Browser-reported heap bytes.
 * @returns Human-readable mebibytes.
 */
function formatMemoryBytes(bytes: number): string {
  return `${(bytes/(1024*1024)).toFixed(1)} MiB`;
}

/** Resolve native details elements without installing additional toggle listeners. */
function findGroup(root: HTMLElement, name: DetailGroup) {
  const details = requireElement(root, `[data-group="${name}"]`, HTMLDetailsElement);
  return {details, list: requireElement(details, 'dl', HTMLElement)};
}

/** Missing counters must remain distinguishable from a measured zero. */
function formatOptionalBytes(bytes: number | null | undefined): string {
  return bytes === null || bytes === undefined ? 'Unavailable' : formatMemoryBytes(bytes);
}
