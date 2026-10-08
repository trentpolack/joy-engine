// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT, KEY_CODE } from 'joy-engine/constants';

const STORAGE_KEY = 'joy-form-workspace-v1';
const PRESETS = {balanced: [0.30, 286], script: [0.43, 270], preview: [0.23, 260]};

/** Owns desktop pane sizing and inspector navigation; preferences never alter a study. */
export class WorkspaceLayout {
  declare root: HTMLElement;
  declare listeners: AbortController;
  declare scriptWidth: number;
  declare inspectorWidth: number;
  declare layout: string;
  declare tab: string;

  /** @param root @param [options] */
  constructor(root: HTMLElement, options: {
    resize?: boolean;
} = {}) {
    this.root = root;
    this.listeners = new AbortController();
    this.scriptWidth = 0.30;
    this.inspectorWidth = 286;
    this.layout = 'balanced';
    this.tab = 'parameters';
    try {
      const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? 'null');
      if(saved && Number.isFinite(saved.scriptWidth) && Number.isFinite(saved.inspectorWidth)) {
        this.scriptWidth = Math.max(0.18, Math.min(0.55, saved.scriptWidth));
        this.inspectorWidth = Math.max(240, Math.min(430, saved.inspectorWidth));
        this.layout = Object.hasOwn(PRESETS, saved.layout) ? saved.layout : 'custom';
      }
    } catch { /* Workspace preferences are optional. */ }
    const signal = this.listeners.signal;
    for(const button of root.querySelectorAll('[data-layout]')) {
      button.addEventListener(BROWSER_EVENT.CLICK, () => {
        const name = ((button) as HTMLElement).dataset.layout ?? 'balanced';
        const preset = PRESETS[ ((name) as keyof typeof PRESETS)];
        [this.scriptWidth, this.inspectorWidth] = preset;
        this.layout = name;
        this.apply();
        this.persist();
      }, {signal});
    }
    for(const button of root.querySelectorAll('[data-inspector-tab]')) {
      button.addEventListener(BROWSER_EVENT.KEYDOWN, event => {
        const key = ((event) as KeyboardEvent).key;
        const tabs = [...root.querySelectorAll('[data-inspector-tab]')];
        let index = tabs.indexOf(button);
        if(!([KEY_CODE.ARROW_LEFT, KEY_CODE.ARROW_RIGHT, KEY_CODE.HOME, KEY_CODE.END] as readonly string[]).includes(key)) {
          return;
        }
        event.preventDefault();
        index = key === KEY_CODE.HOME ? 0 : key === KEY_CODE.END ? tabs.length - 1 : (index + (key === KEY_CODE.ARROW_RIGHT ? 1 : -1) + tabs.length)%tabs.length;
        const next = ((tabs[index]) as HTMLButtonElement);
        this.selectTab(next.dataset.inspectorTab ?? 'parameters');
        next.focus();
      }, {signal});
      button.addEventListener(BROWSER_EVENT.CLICK, () => this.selectTab(((button) as HTMLElement).dataset.inspectorTab ?? 'parameters'), {signal});
    }
    if(options.resize !== false) {
      this.connectResize('script', root.querySelector('.editor-panel'));
      this.connectResize('inspector', root.querySelector('.inspector'));
    }
    window.addEventListener(BROWSER_EVENT.RESIZE, () => this.apply(), {signal});
    this.apply();
    this.selectTab('parameters');
  }
  apply() {
    const workbench = ((this.root.querySelector('.workbench')) as HTMLElement);
    const width = workbench.clientWidth;
    const inspector = Math.min(this.inspectorWidth, Math.max(240, width - 580));
    const script = Math.min(Math.max(240, width*this.scriptWidth), Math.max(240, width - inspector - 300));
    workbench.style.setProperty('--script-width', `${script}px`);
    workbench.style.setProperty('--inspector-width', `${inspector}px`);
    for(const button of this.root.querySelectorAll('[data-layout]')) {
      const active = ((button) as HTMLElement).dataset.layout === this.layout;
      button.classList.toggle('selected', active);
      button.setAttribute('aria-pressed', String(active));
    }
    for(const handle of this.root.querySelectorAll('[data-resize]')) {
      handle.setAttribute('aria-valuenow', String(Math.round(((handle) as HTMLElement).dataset.resize === 'script' ? script : inspector)));
    }
  }
  /** @param tab */
  selectTab(tab: string) {
    this.tab = tab;
    for(const button of this.root.querySelectorAll('[data-inspector-tab]')) {
      const active = ((button) as HTMLElement).dataset.inspectorTab === tab;
      button.classList.toggle('selected', active);
      button.setAttribute('aria-selected', String(active));
       ((button) as HTMLElement).tabIndex = active ? 0 : -1;
    }
    for(const panel of this.root.querySelectorAll('[data-inspector-panel]')) {
       ((panel) as HTMLElement).hidden = ((panel) as HTMLElement).dataset.inspectorPanel !== tab;
    }
  }
  /** @param side @param panel */
  connectResize(side: 'script' | 'inspector', panel: Element | null) {
    if(!panel) {
      return;
    }
    const handle = document.createElement('div');
    handle.className = `pane-resize ${side}`;
    handle.dataset.resize = side;
    handle.tabIndex = 0;
    handle.setAttribute('role', 'separator');
    handle.setAttribute('aria-orientation', 'vertical');
    handle.setAttribute('aria-label', `Resize ${side} panel`);
    handle.setAttribute('aria-valuemin', '240');
    handle.title = 'Drag to resize · Arrow keys adjust · Double-click resets';
    panel.append(handle);
    const signal = this.listeners.signal;
    let start = 0, initial = 0, dragging = false;
    /** @param delta */
    const adjust = (delta: number) => {
      const width = ((this.root.querySelector('.workbench')) as HTMLElement).clientWidth;
      if(side === 'script') {
        this.scriptWidth = Math.max(0.18, Math.min(0.55, (initial + delta)/width));
      } else {
        this.inspectorWidth = Math.max(240, Math.min(430, initial - delta));
      }
      this.layout = 'custom';
      this.apply();
    };
    handle.addEventListener(BROWSER_EVENT.POINTERDOWN, event => {
      dragging = true;
      start = event.clientX;
      initial = panel.clientWidth;
      handle.setPointerCapture(event.pointerId);
      event.preventDefault();
    }, {signal});
    handle.addEventListener(BROWSER_EVENT.POINTERMOVE, event => {
      if(dragging) {
        adjust(event.clientX - start);
      }
    }, {signal});
    const finish = () => {
      dragging = false;
      this.persist();
    };
    handle.addEventListener(BROWSER_EVENT.POINTERUP, finish, {signal});
    handle.addEventListener(BROWSER_EVENT.POINTERCANCEL, finish, {signal});
    handle.addEventListener('dblclick', () => {
      [this.scriptWidth, this.inspectorWidth] = PRESETS.balanced;
      this.layout = 'balanced';
      this.apply();
      this.persist();
    }, {signal});
    handle.addEventListener(BROWSER_EVENT.KEYDOWN, event => {
      if(event.key === KEY_CODE.ARROW_LEFT || event.key === KEY_CODE.ARROW_RIGHT) {
        event.preventDefault();
        initial = panel.clientWidth;
        adjust(event.key === KEY_CODE.ARROW_LEFT ? -20 : 20);
        this.persist();
      }
    }, {signal});
  }
  persist() {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify({scriptWidth: this.scriptWidth, inspectorWidth: this.inspectorWidth, layout: this.layout}));
    } catch { /* A blocked preference store must not block editing. */ }
  }
  destroy() {
    this.listeners.abort();
  }
}
