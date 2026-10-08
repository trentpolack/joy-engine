// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { INPUT_BINDING_TYPE, INPUT_CONTEXT } from './constants.ts';
import { BROWSER_EVENT } from '../constants.ts';

export type InputBinding = {
    key: string;
} | {
    pointer: number;
} | {
    button: number;
} | {
    virtual: string;
};
export interface InputAction {
  bindings: readonly InputBinding[];
  contexts?: readonly string[];
}
export interface InputAxis {
  positive?: string;
  negative?: string;
  gamepadAxis?: number;
  deadZone?: number;
  contexts?: readonly string[];
}
export interface InputActionState {
  down: boolean;
  pressed: boolean;
  released: boolean;
  value: number;
}
export interface InputSnapshot {
  actions: Record<string, InputActionState>;
  axes: Record<string, number>;
}

export interface InputManagerOptions {
  actions: Record<string, InputAction>;
  axes?: Record<string, InputAxis>;
  target?: EventTarget;
  documentTarget?: EventTarget;
  pointerTarget?: EventTarget;
  getGamepads?: () => readonly (Pick<Gamepad, 'connected' | 'axes' | 'buttons'> | null)[];
  onPress?: (name: string) => void;
}

/**
 * Own device state and subscriptions for one player. Actions aggregate by maximum
 * value; axes choose the strongest digital or analog input. Call update once per
 * simulation frame to consume edges, including taps completed between frames.
 */
export class InputManager {
  declare actions: { [k: string]: { bindings: ({ key: string; } | { pointer: number; } | { button: number; } | { virtual: string; })[]; contexts: string[]; }; };
  declare axes: { [k: string]: { positive?: string; negative?: string; gamepadAxis?: number; deadZone?: number; contexts?: readonly string[]; }; };
  declare contexts: Set<string>;
  declare keys: Set<string>;
  declare pointers: Map<number, number>;
  declare virtual: Map<string, number>;
  declare states: Record<string, InputActionState>;
  declare pointer: { x: number; y: number; active: boolean; };
  declare getGamepads: () => readonly (Pick<Gamepad, "connected" | "axes" | "buttons"> | null)[];
  declare onPress: (name: string) => void;
  declare typing: boolean;
  declare blurred: boolean;
  declare hidden: boolean;
  declare destroyed: boolean;
  declare blockGamepads: boolean;
  declare releaseGeneration: number;
  declare gamepads: readonly (Pick<Gamepad, "axes" | "connected" | "buttons"> | null)[];
  declare cleanups: Set<() => void>;

  /**
   * @param options
   * @param options.actions Copied binding definitions.
   * @param [options.axes] Copied axis definitions.
   * @param [options.target] Borrowed keyboard/window event source.
   * @param [options.documentTarget] Borrowed focus/visibility source.
   * @param [options.pointerTarget] Borrowed pointer surface.
   * @param [options.getGamepads] Poll adapter; all connected pads contribute.
   * @param [options.onPress] Immediate edge callback; never repeats while held.
   */
  constructor({actions, axes = {}, target = window, documentTarget = document, pointerTarget = target,
    getGamepads = () => navigator.getGamepads(), onPress = () => {}}: InputManagerOptions) {
    this.actions = Object.fromEntries(Object.entries(actions).map(([name, action]) =>
      [name, {...action, bindings: action.bindings.map(binding => ({...binding})), contexts: [...(action.contexts ?? [INPUT_CONTEXT.GAMEPLAY])]}]));
    this.axes = Object.fromEntries(Object.entries(axes).map(([name, axis]) => [name, {...axis}]));
    this.contexts = new Set([INPUT_CONTEXT.GAMEPLAY]);
    this.keys = new Set();
    /** Pointer ID to button. */
    this.pointers = new Map();

    this.virtual = new Map();

    this.states = Object.fromEntries(Object.keys(actions).map(name => [name, {down: false, pressed: false, released: false, value: 0}]));
    /**
     * Client pixels; borrowed read-only by consumers. */
    this.pointer = {x: 0, y: 0, active: false};
    this.getGamepads = getGamepads;
    this.onPress = onPress;
    this.typing = false;
    this.blurred = false;
    this.hidden = false;
    this.destroyed = false;
    this.blockGamepads = false;
    this.releaseGeneration = 0;

    this.gamepads = [];

    this.cleanups = new Set();
    this.listen(target, BROWSER_EVENT.KEYDOWN, event => {
      const key = ((event) as KeyboardEvent);
      if(key.repeat || isTypingTarget(event.target) || this.suspended) {
        return;
      }
      if(this.boundKey(key.code)) {
        key.preventDefault();
      }
      this.keys.add(key.code);
      this.refresh();
    });
    this.listen(target, BROWSER_EVENT.KEYUP, event => {
      this.keys.delete(((event) as KeyboardEvent).code);
      this.refresh();
    });
    this.listen(target, BROWSER_EVENT.BLUR, () => { this.blurred = true; this.clear(); });
    this.listen(target, BROWSER_EVENT.FOCUS, () => { this.blurred = false; });
    this.listen(documentTarget, BROWSER_EVENT.VISIBILITYCHANGE, () => {
      this.hidden = 'hidden' in documentTarget && Boolean(documentTarget.hidden);
      this.clear();
    });
    this.listen(documentTarget, BROWSER_EVENT.FOCUSIN, event => {
      this.typing = isTypingTarget(event.target);
      if(this.typing) {
        this.clear();
      }
    });
    this.listen(documentTarget, BROWSER_EVENT.FOCUSOUT, () => { this.typing = false; });
    this.listen(pointerTarget, BROWSER_EVENT.POINTERMOVE, event => this.movePointer(((event) as PointerEvent)));
    this.listen(pointerTarget, BROWSER_EVENT.POINTERDOWN, event => {
      if(this.suspended || isTypingTarget(event.target)) {
        return;
      }
      const pointer = ((event) as PointerEvent);
      this.movePointer(pointer);
      this.pointers.set(pointer.pointerId, pointer.button);
      this.refresh();
    });
    for(const type of [BROWSER_EVENT.POINTERUP, BROWSER_EVENT.POINTERCANCEL, BROWSER_EVENT.LOSTPOINTERCAPTURE]) {
      this.listen(target, type, event => {
        this.pointers.delete(((event) as PointerEvent).pointerId);
        this.refresh();
      });
    }
  }

  /**
   * Whether focus or visibility prevents all gameplay devices from contributing. */
  get suspended() {
    return this.typing || this.blurred || this.hidden || this.destroyed;
  }

  /**
   * Poll pads and return independently owned action/axis snapshots, consuming edges.
   * Analog sticks use a rescaled dead zone (default 0.15). Values have no time units.
   */
  update(): InputSnapshot {
    const pads = this.destroyed ? [] : this.getGamepads();
    if(this.blockGamepads && !pads.some(pad => pad?.connected &&
      (pad.buttons.some(button => button.value > 0.15) || pad.axes.some(axis => Math.abs(axis) > 0.15)))) {
      this.blockGamepads = false;
    }
    this.gamepads = this.suspended || this.blockGamepads ? [] : pads;
    this.refresh();

    const snapshot: InputSnapshot = {actions: {}, axes: {}};
    for(const [name, state] of Object.entries(this.states)) {
      snapshot.actions[name] = {...state};
      state.pressed = false;
      state.released = false;
    }
    for(const [name, axis] of Object.entries(this.axes)) {
      let value = (this.states[axis.positive ?? '']?.value ?? 0) - (this.states[axis.negative ?? '']?.value ?? 0);
      if(this.suspended || !this.enabled(axis.contexts)) {
        snapshot.axes[name] = 0;
        continue;
      }
      for(const pad of this.gamepads) {
        const raw = pad?.connected && axis.gamepadAxis !== undefined ? (pad.axes[axis.gamepadAxis] ?? 0) : 0;
        const deadZone = Math.max(0, Math.min(0.99, axis.deadZone ?? 0.15));
        const analog = Math.sign(raw)*Math.max(0, (Math.abs(raw) - deadZone)/(1 - deadZone));
        if(Math.abs(analog) > Math.abs(value)) {
          value = analog;
        }
      }
      snapshot.axes[name] = Math.max(-1, Math.min(1, value));
    }
    return snapshot;
  }

  /**
   * Replace active contexts and release previous holds. Pads must return to neutral.
   * @param contexts Empty disables all actions.
   */
  setContexts(contexts: readonly string[]) {
    this.contexts = new Set(contexts);
    this.clear();
  }

  /**
   * Replace one action's copied bindings; old device holds are released.
   * @param name
   * @param bindings
   */
  rebind(name: string, bindings: readonly InputBinding[]) {
    if(!Object.hasOwn(this.actions, name)) {
      throw new RangeError(`Unknown input action "${name}".`);
    }
    this.actions[name].bindings = bindings.map(binding => ({...binding}));
    this.clear();
  }

  /**
   * Set a named virtual control (0 releases, 1 fully presses); supports touch adapters.
   * @param name
   * @param value Finite value between zero and one.
   */
  setVirtual(name: string, value: number) {
    if(!Number.isFinite(value) || value < 0 || value > 1) {
      throw new RangeError('Virtual input must be between zero and one.');
    }
    if(this.suspended) {
      return;
    }
    this.virtual.set(name, value);
    this.refresh();
  }

  /**
   * Bind a borrowed touch button with pointer capture and multi-touch ownership.
   * @param button
   * @param name Virtual binding name; use one button per name.
   * @returns Idempotent unbind, also called by destroy.
   */
  bindVirtualButton(button: EventTarget & {
    setPointerCapture?: (id: number) => void;
}, name: string): () => void {

    const pointers: Set<number> = new Set();
    let generation = this.releaseGeneration;
    const down = (event: Event) => {
      const pointer = ((event) as PointerEvent);
      event.preventDefault();
      if(generation !== this.releaseGeneration) {
        pointers.clear();
        generation = this.releaseGeneration;
      }
      pointers.add(pointer.pointerId);
      button.setPointerCapture?.(pointer.pointerId);
      this.setVirtual(name, 1);
    };
    const up = (event: Event) => {
      if(generation !== this.releaseGeneration) {
        pointers.clear();
        generation = this.releaseGeneration;
      }
      pointers.delete(((event) as PointerEvent).pointerId);
      this.setVirtual(name, pointers.size ? 1 : 0);
    };
    button.addEventListener(BROWSER_EVENT.POINTERDOWN, down);
    for(const type of [BROWSER_EVENT.POINTERUP, BROWSER_EVENT.POINTERCANCEL, BROWSER_EVENT.LOSTPOINTERCAPTURE]) {
      button.addEventListener(type, up);
    }
    const cleanup = () => {
      button.removeEventListener(BROWSER_EVENT.POINTERDOWN, down);
      for(const type of [BROWSER_EVENT.POINTERUP, BROWSER_EVENT.POINTERCANCEL, BROWSER_EVENT.LOSTPOINTERCAPTURE]) {
        button.removeEventListener(type, up);
      }
      pointers.clear();
      this.virtual.delete(name);
      this.refresh();
      this.cleanups.delete(cleanup);
    };
    this.cleanups.add(cleanup);
    return cleanup;
  }

  /**
   * Release all holds; gamepads must pass through neutral before they resume. */
  clear() {
    this.releaseGeneration++;
    this.keys.clear();
    this.pointers.clear();
    this.virtual.clear();
    this.gamepads = [];
    this.blockGamepads = true;
    this.refresh();
  }

  /**
   * Release state and all owned listeners; repeated calls are harmless. */
  destroy() {
    if(this.destroyed) {
      return;
    }
    this.destroyed = true;
    this.clear();
    for(const cleanup of this.cleanups) {
      cleanup();
    }
    this.cleanups.clear();
  }

  /** @private @param [contexts] */
  private enabled(contexts: readonly string[] = [INPUT_CONTEXT.GAMEPLAY]) {
    return contexts.some(context => this.contexts.has(context));
  }

  /** @private @param key */
  private boundKey(key: string) {
    return Object.values(this.actions).some(action => this.enabled(action.contexts) &&
      action.bindings.some(binding => INPUT_BINDING_TYPE.KEY in binding && binding.key === key));
  }

  /** @private @param binding */
  private bindingValue(binding: InputBinding) {
    if(INPUT_BINDING_TYPE.KEY in binding) {
      return Number(this.keys.has(binding.key));
    }
    if(INPUT_BINDING_TYPE.POINTER in binding) {
      return Number([...this.pointers.values()].includes(binding.pointer));
    }
    if(INPUT_BINDING_TYPE.VIRTUAL in binding) {
      return this.virtual.get(binding.virtual) ?? 0;
    }
    return Math.max(0, ...this.gamepads.map(pad => pad?.connected ? (pad.buttons[binding.button]?.value ?? 0) : 0));
  }

  /**
   * Commit all states before callbacks, allowing callbacks to clear or change context. @private */
  private refresh() {
    const presses = [];
    for(const [name, action] of Object.entries(this.actions)) {
      const value = !this.suspended && this.enabled(action.contexts) ? Math.max(0, ...action.bindings.map(binding => this.bindingValue(binding))) : 0;
      const state = this.states[name];
      const down = value > 0.15;
      if(down && !state.down) {
        state.pressed = true;
        presses.push(name);
      } else if(!down && state.down) {
        state.released = true;
      }
      state.down = down;
      state.value = value;
    }
    for(const name of presses) {
      if(this.states[name].down) {
        this.onPress(name);
      }
    }
  }

  /** @private @param event */
  private movePointer(event: PointerEvent) {
    this.pointer = {x: event.clientX, y: event.clientY, active: true};
  }

  /** @private @param target @param type @param listener */
  private listen(target: EventTarget, type: string, listener: EventListener) {
    target.addEventListener(type, listener);
    this.cleanups.add(() => target.removeEventListener(type, listener));
  }
}

/**
 * Include descendants of editable containers, selects and explicit textbox roles.
 * @param target
 */
function isTypingTarget(target: EventTarget | null) {
  return Boolean(target && 'closest' in target && typeof target.closest === 'function' &&
    target.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"]), [role="textbox"]'));
}
