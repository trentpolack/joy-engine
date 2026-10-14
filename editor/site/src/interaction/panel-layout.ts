// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT, KEY_CODE } from 'joy-engine/constants';
import '../../css/panel-layout.css';

/**
 * Owns bounded editor panel preferences and dividers. Existing document nodes stay mounted. */
export class PanelLayout {
  declare root: HTMLElement;
  declare panels: { left: HTMLElement; right: HTMLElement; center: HTMLElement; key: string; leftWidth?: number; rightWidth?: number; visibilityHost?: HTMLElement; maximizeMode?: "viewport" | "workspace"; onMaximize?: (value: boolean) => void; };
  declare listeners: AbortController;
  declare leftWidth: number;
  declare rightWidth: number;
  declare maximized: boolean;
  declare collapsed: {left: boolean; center: boolean; right: boolean};
  declare handles: HTMLElement[];
  declare visibilityControls: HTMLButtonElement[];
  declare maximize: HTMLButtonElement;
  declare observer: ResizeObserver;

  /** @param root @param panels */
  constructor(root: HTMLElement, panels: {
    left: HTMLElement;
    right: HTMLElement;
    center: HTMLElement;
    key: string;
    leftWidth?: number;
    rightWidth?: number;
    visibilityHost?: HTMLElement;
    maximizeMode?: 'viewport' | 'workspace';
    onMaximize?: (value: boolean) => void;
}) {
    this.root = root;
    this.panels = panels;
    this.listeners = new AbortController();
    this.leftWidth = panels.leftWidth ?? 280;
    this.rightWidth = panels.rightWidth ?? 260;
    this.maximized = false;
    this.collapsed = {left: false, center: false, right: false};
    this.handles = [];
    this.visibilityControls = [];
    try {
      const saved = JSON.parse(localStorage.getItem(panels.key) ?? '{}');
      if(Number.isFinite(saved.left) && Number.isFinite(saved.right)) {
        this.leftWidth = Math.max(160, Math.min(600, saved.left));
        this.rightWidth = Math.max(220, Math.min(480, saved.right));
      }
      this.collapsed.left = saved.leftCollapsed === true;
      this.collapsed.right = saved.rightCollapsed === true;
      this.collapsed.center = Boolean(panels.visibilityHost) && saved.centerCollapsed === true;
      if(Object.values(this.collapsed).every(Boolean)) {
        this.collapsed.center = false;
      }
    } catch { /* Optional preferences must never prevent authoring. */ }
    root.classList.add('joy-panel-layout');
    panels.left.classList.add('layout-left');
    panels.right.classList.add('layout-right');
    panels.center.classList.add('layout-center');
    this.createHandle('left');
    this.createHandle('right');
    this.maximize = document.createElement('button');
    this.maximize.type = 'button';
    this.maximize.className = 'layout-maximize';
    this.maximize.textContent = 'Maximize';
    this.maximize.setAttribute('aria-label', panels.maximizeMode === 'workspace' ? 'Maximize lab workspace' : 'Maximize viewport');
    this.maximize.setAttribute('aria-pressed', 'false');
    const toolbar = panels.center.querySelector('.preview-actions, .viewport-toolbar') ?? panels.center;
    const visibilityHost = panels.visibilityHost ?? toolbar;
    this.createVisibilityControl('left', visibilityHost);
    if(panels.visibilityHost) {
      this.createVisibilityControl('center', visibilityHost);
    }
    this.createVisibilityControl('right', visibilityHost);
    toolbar.append(this.maximize);
    this.maximize.addEventListener(BROWSER_EVENT.CLICK, () => {
      this.setMaximized(!this.maximized);
      panels.onMaximize?.(this.maximized);
    }, {signal: this.listeners.signal});
    window.addEventListener(BROWSER_EVENT.KEYDOWN, event => {
      const target = event.target;
      if(event.key === KEY_CODE.ESCAPE && this.maximized && !event.defaultPrevented && !(target instanceof HTMLElement && target.closest('dialog, input, textarea, select, .cm-editor'))) {
        this.setMaximized(false);
        panels.onMaximize?.(false);
      }
    }, {signal: this.listeners.signal});
    this.observer = new ResizeObserver(() => this.apply());
    this.observer.observe(root);
    this.apply();
  }

  /** Update presentation only; document nodes and saved widths remain owned in place.
   * @param maximized
   */
  setMaximized(maximized: boolean) {
    this.maximized = maximized;
    const workspace = this.panels.maximizeMode === 'workspace';
    this.root.classList.toggle('viewport-maximized', maximized && !workspace);
    this.root.closest('#app')?.classList.toggle('lab-workspace-maximized', maximized && workspace);
    this.maximize.textContent = maximized ? 'Restore' : 'Maximize';
    this.maximize.setAttribute('aria-label', maximized ? 'Restore workspace' : workspace ? 'Maximize lab workspace' : 'Maximize viewport');
    this.maximize.setAttribute('aria-pressed', String(maximized));
    this.apply();
  }

  /** Re-clamp displayed widths after parent-window changes without overwriting preferences. */
  apply() {
    const width = this.root.clientWidth;
    const left = Math.max(160, Math.min(this.leftWidth, width*0.36));
    const right = Math.max(220, Math.min(this.rightWidth, width - left - 280));
    this.root.style.setProperty('--edit-width', this.collapsed.left ? '0px' : `${left}px`);
    this.root.style.setProperty('--inspect-width', this.collapsed.right ? '0px' : `${right}px`);
    this.root.classList.toggle('layout-left-collapsed', this.collapsed.left);
    this.root.classList.toggle('layout-right-collapsed', this.collapsed.right);
    this.root.classList.toggle('layout-center-collapsed', this.collapsed.center);
    this.root.style.setProperty('--visible-panel-count', String(Object.values(this.collapsed).filter(value => !value).length));
    for(const side of ['left', 'center', 'right'] as const) {
      this.panels[side].hidden = this.collapsed[side];
    }
    for(const control of this.visibilityControls) {
      const side = control.dataset.side as 'left' | 'center' | 'right';
      const visible = !this.collapsed[side];
      const label = side === 'left' ? 'script' : side === 'center' ? 'preview' : 'inspector';
      control.setAttribute('aria-pressed', String(visible));
      control.setAttribute('aria-label', `${visible ? 'Hide' : 'Show'} ${label} panel`);
      control.disabled = visible && Object.values(this.collapsed).filter(value => !value).length === 1;
    }
    this.root.classList.toggle('layout-compact', width < 800);
    for(const handle of this.handles) {
      const side = handle.dataset.side === 'left' ? 'left' : 'right';
      const panel = this.panels[side];
      handle.hidden = this.collapsed[side] || this.collapsed.center;
      handle.style.left = `${side === 'left' ? panel.offsetLeft + panel.clientWidth - 5 : panel.offsetLeft - 5}px`;
      handle.style.top = `${panel.offsetTop}px`;
      handle.style.height = `${panel.clientHeight}px`;
      handle.setAttribute('aria-valuenow', String(Math.round(panel.clientWidth)));
      handle.setAttribute('aria-valuemax', String(side === 'left' ? 600 : 480));
    }
  }

  /** Release layout listeners without changing document or camera state. */
  destroy() {
    this.listeners.abort();
    this.observer.disconnect();
    this.handles.forEach(handle => handle.remove());
    this.visibilityControls.forEach(control => control.remove());
    this.maximize.remove();
    document.body.classList.remove('panel-resizing');
  }

  /** @private @param side @param toolbar */
  private createVisibilityControl(side: 'left' | 'center' | 'right', toolbar: Element) {
    const label = side === 'left' ? 'Script' : side === 'center' ? 'Preview' : 'Inspector';
    const control = document.createElement('button');
    control.type = 'button';
    control.className = 'layout-panel-toggle';
    control.dataset.side = side;
    control.textContent = label;
    control.title = `Show or hide the ${label.toLowerCase()} panel`;
    control.addEventListener(BROWSER_EVENT.CLICK, () => {
      this.collapsed[side] = !this.collapsed[side];
      this.persist();
      this.apply();
    }, {signal: this.listeners.signal});
    toolbar.append(control);
    this.visibilityControls.push(control);
  }

  private persist() {
    try {
      localStorage.setItem(this.panels.key, JSON.stringify({
        left: this.leftWidth,
        right: this.rightWidth,
        leftCollapsed: this.collapsed.left,
        centerCollapsed: this.collapsed.center,
        rightCollapsed: this.collapsed.right
      }));
    } catch { /* Keep the working in-memory layout when storage is unavailable. */ }
  }

  /** @private @param side */
  private createHandle(side: 'left' | 'right') {
    const handle = document.createElement('div');
    handle.className = 'layout-divider';
    handle.dataset.side = side;
    handle.tabIndex = 0;
    handle.setAttribute('role', 'separator');
    handle.setAttribute('aria-orientation', 'vertical');
    handle.setAttribute('aria-label', side === 'left' ? 'Resize editing panel' : 'Resize Inspector');
    handle.setAttribute('aria-valuemin', side === 'left' ? '160' : '220');
    handle.title = 'Drag or use arrow keys to resize';
    this.root.append(handle);
    this.handles.push(handle);
    const signal = this.listeners.signal;
    let start = 0, initial = 0;

    let pointerId: number | null = null;
    const adjust = (delta: number) => {
      if(side === 'left') {
        this.leftWidth = Math.max(160, Math.min(600, initial + delta));
      }
      else {
        this.rightWidth = Math.max(220, Math.min(480, initial - delta));
      }
      this.apply();
    };
    handle.addEventListener(BROWSER_EVENT.POINTERDOWN, event => {
      if(event.button !== 0) {
        return;
      }
      start = event.clientX;
      initial = this.panels[side].clientWidth;
      pointerId = event.pointerId;
      handle.setPointerCapture(event.pointerId);
      document.body.classList.add('panel-resizing');
      event.preventDefault();
    }, {signal});
    handle.addEventListener(BROWSER_EVENT.POINTERMOVE, event => {
      if(handle.hasPointerCapture(event.pointerId)) {
        adjust(event.clientX - start);
      }
    }, {signal});
    const finish = () => {
      const id = pointerId;
      pointerId = null;
      if(id !== null && handle.hasPointerCapture(id)) {
        handle.releasePointerCapture(id);
      }
      document.body.classList.remove('panel-resizing');
      this.persist();
    };
    for(const type of [BROWSER_EVENT.POINTERUP, BROWSER_EVENT.POINTERCANCEL, BROWSER_EVENT.LOSTPOINTERCAPTURE]) { handle.addEventListener(type, finish, {signal}); }
    window.addEventListener(BROWSER_EVENT.BLUR, finish, {signal});
    handle.addEventListener(BROWSER_EVENT.KEYDOWN, event => {
      if(event.key !== KEY_CODE.ARROW_LEFT && event.key !== KEY_CODE.ARROW_RIGHT) {
        return;
      }
      event.preventDefault();
      initial = this.panels[side].clientWidth;
      adjust(event.key === KEY_CODE.ARROW_LEFT ? -20 : 20);
      finish();
    }, {signal});
  }
}
