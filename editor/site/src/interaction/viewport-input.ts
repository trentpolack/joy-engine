// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { VIEWPORT_GESTURE } from './constants.ts';
import { BROWSER_EVENT } from 'joy-engine/constants';

const DRAG_THRESHOLD_PIXELS = 4;
const LINE_HEIGHT_PIXELS = 16;

/**
 * Owns one captured camera gesture and canvas shortcuts; borrows callbacks and DOM.
 * Deltas are logical CSS pixels. Camera and authored state remain with the caller. */
export class ViewportInput {
  declare canvas: HTMLCanvasElement;
  declare actions: { drag: (mode: typeof VIEWPORT_GESTURE.PAN | typeof VIEWPORT_GESTURE.ORBIT, dx: number, dy: number) => void; zoom: (delta: number) => void; shortcut: (key: string) => void; click?: (event: PointerEvent) => void; };
  declare listeners: AbortController;
  declare pointer: { id: number; x: number; y: number; lastX: number; lastY: number; mode: typeof VIEWPORT_GESTURE.PAN | typeof VIEWPORT_GESTURE.ORBIT; moved: boolean; click: boolean; } | null;

  /** @param canvas
   * @param actions */
  constructor(canvas: HTMLCanvasElement, actions: {
    drag: (mode: typeof VIEWPORT_GESTURE.PAN | typeof VIEWPORT_GESTURE.ORBIT, dx: number, dy: number) => void;
    zoom: (delta: number) => void;
    shortcut: (key: string) => void;
    click?: (event: PointerEvent) => void;
}) {
    this.canvas = canvas;
    this.actions = actions;
    this.listeners = new AbortController();

    this.pointer = null;
    const signal = this.listeners.signal;
    canvas.addEventListener(BROWSER_EVENT.CONTEXTMENU, event => event.preventDefault(), {signal});
    canvas.addEventListener(BROWSER_EVENT.POINTERDOWN, event => this.begin(event), {signal});
    canvas.addEventListener(BROWSER_EVENT.POINTERMOVE, event => this.move(event), {signal});
    canvas.addEventListener(BROWSER_EVENT.POINTERUP, event => this.end(event), {signal});
    for(const type of [BROWSER_EVENT.POINTERCANCEL, BROWSER_EVENT.LOSTPOINTERCAPTURE]) {
      canvas.addEventListener(type, () => this.cancel(), {signal});
    }
    window.addEventListener(BROWSER_EVENT.BLUR, () => this.cancel(), {signal});
    canvas.addEventListener(BROWSER_EVENT.WHEEL, event => {
      if(!canvas.clientWidth || !canvas.clientHeight) {
        return;
      }
      event.preventDefault();
      actions.zoom(wheelPixels(event, canvas.clientHeight));
    }, {signal, passive: false});
    canvas.addEventListener(BROWSER_EVENT.KEYDOWN, event => {
      if(canvas.ownerDocument.activeElement !== canvas || event.isComposing || event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) {
        return;
      }
      const key = event.key.toLowerCase();
      if(key === 'f' || key === '0' || key.startsWith('arrow')) {
        if(event.repeat && (key === 'f' || key === '0')) {
          return;
        }
        event.preventDefault();
        actions.shortcut(key);
      }
    }, {signal});
  }

  /**
   * End input ownership before the caller releases its camera/renderer. */
  destroy() {
    this.cancel();
    this.listeners.abort();
  }

  /** @private @param event */
  private begin(event: PointerEvent) {
    if(this.pointer || event.button < 0 || event.button > 2) {
      return;
    }
    this.canvas.focus({preventScroll: true});
    this.canvas.setPointerCapture(event.pointerId);
    this.pointer = {id: event.pointerId, x: event.clientX, y: event.clientY,
      lastX: event.clientX, lastY: event.clientY,
      mode: event.button !== 0 || event.shiftKey ? VIEWPORT_GESTURE.PAN : VIEWPORT_GESTURE.ORBIT, moved: false,
      click: event.button === 0 && !event.shiftKey && !event.altKey && !event.ctrlKey && !event.metaKey};
  }

  /** @private @param event */
  private move(event: PointerEvent) {
    const pointer = this.pointer;
    if(!pointer || pointer.id !== event.pointerId) {
      return;
    }
    const dx = event.clientX - pointer.lastX;
    const dy = event.clientY - pointer.lastY;
    pointer.lastX = event.clientX;
    pointer.lastY = event.clientY;
    pointer.moved ||= Math.hypot(event.clientX - pointer.x, event.clientY - pointer.y) >= DRAG_THRESHOLD_PIXELS;
    if(pointer.moved) {
      this.actions.drag(pointer.mode, dx, dy);
    }
  }

  /** @private @param event */
  private end(event: PointerEvent) {
    const pointer = this.pointer;
    if(!pointer || pointer.id !== event.pointerId) {
      return;
    }
    this.cancel();
    if(pointer.click && !pointer.moved && Math.hypot(event.clientX - pointer.x, event.clientY - pointer.y) < DRAG_THRESHOLD_PIXELS) {
      this.actions.click?.(event);
    }
  }

  /** @private */
  private cancel() {
    const pointer = this.pointer;
    this.pointer = null;
    if(pointer && this.canvas.hasPointerCapture(pointer.id)) {
      this.canvas.releasePointerCapture(pointer.id);
    }
  }
}

/**
 * Normalize wheel units without changing the caller's zoom sensitivity.
 * @param event @param pageHeight CSS pixels. */
export function wheelPixels(event: WheelEvent, pageHeight: number) {
  return event.deltaY*(event.deltaMode === 1 ? LINE_HEIGHT_PIXELS : event.deltaMode === 2 ? pageHeight : 1);
}
