// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT } from './constants.ts';

/**
 * Tracks held keys and a pointer, with optional handlers for discrete actions.
 * Pointer coordinates stay in client pixels; games with inset or scaled arenas
 * must convert them to their own coordinate space. Call destroy when disposing.
 */
export class InputController {
  declare keys: Set<string>;
  declare pointer: { x: number; y: number; down: boolean; };
  declare pointerTarget: HTMLElement | Window;
  declare handlers: Map<string, (event: KeyboardEvent | PointerEvent) => void>;
  declare onKeyDown: (event: KeyboardEvent) => void;
  declare onKeyUp: (event: KeyboardEvent) => boolean;
  declare onPointerMove: (event: Event) => void;
  declare onPointerDown: (event: Event) => void;
  declare onPointerUp: () => void;

  /**
   * Register owned listeners on the borrowed pointer target and window.
   * @param pointerTarget
   */
  constructor(pointerTarget: HTMLElement | Window = window) {

    this.keys = new Set();
    this.pointer = { x: 0, y: 0, down: false };
    this.pointerTarget = pointerTarget;

    this.handlers = new Map();

    /**
     * @param event
     */
    this.onKeyDown = (event: KeyboardEvent) => {
      this.keys.add(event.code);
      this.handlers.get(event.code)?.(event);
    };
    /**
     * @param event
     */
    this.onKeyUp = (event: KeyboardEvent) => this.keys.delete(event.code);
    /**
     * @param event
     */
    this.onPointerMove = (event: Event) => {
      if(!(event instanceof PointerEvent)) {
        return;
      }
      this.pointer.x = event.clientX;
      this.pointer.y = event.clientY;
    };
    /**
     * @param event
     */
    this.onPointerDown = (event: Event) => {
      if(!(event instanceof PointerEvent)) {
        return;
      }
      this.pointer.down = true;
      this.onPointerMove(event);
      this.handlers.get('PointerDown')?.(event);
    };
    this.onPointerUp = () => {
      this.pointer.down = false;
    };

    window.addEventListener(BROWSER_EVENT.KEYDOWN, this.onKeyDown);
    window.addEventListener(BROWSER_EVENT.KEYUP, this.onKeyUp);
    window.addEventListener(BROWSER_EVENT.POINTERUP, this.onPointerUp);
    pointerTarget.addEventListener(BROWSER_EVENT.POINTERMOVE, this.onPointerMove);
    pointerTarget.addEventListener(BROWSER_EVENT.POINTERDOWN, this.onPointerDown);
  }

  /**
   * Test whether any requested KeyboardEvent.code is currently held.
   * @param codes
   */
  isDown(...codes: string[]) {
    return codes.some((code) => this.keys.has(code));
  }

  /**
   * Assign one action handler, replacing any previous binding for the same code.
   * @param code KeyboardEvent.code or PointerDown.
   * @param handler Receives the original browser event, including key repeats.
   */
  bind(code: string, handler: (event: KeyboardEvent | PointerEvent) => void) {
    this.handlers.set(code, handler);
  }

  /** Remove owned listeners; the borrowed pointer target remains in place. */
  destroy() {
    window.removeEventListener(BROWSER_EVENT.KEYDOWN, this.onKeyDown);
    window.removeEventListener(BROWSER_EVENT.KEYUP, this.onKeyUp);
    window.removeEventListener(BROWSER_EVENT.POINTERUP, this.onPointerUp);
    this.pointerTarget.removeEventListener(BROWSER_EVENT.POINTERMOVE, this.onPointerMove);
    this.pointerTarget.removeEventListener(BROWSER_EVENT.POINTERDOWN, this.onPointerDown);
  }
}
