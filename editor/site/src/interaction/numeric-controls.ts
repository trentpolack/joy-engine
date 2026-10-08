// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT, KEY_CODE } from 'joy-engine/constants';
import '../../css/property-controls.css';
const HISTORY_LIMIT = 30;
const MAX_SNAPSHOT_BYTES = 1024*1024;
const FINE_FACTOR = 0.1;
const COARSE_FACTOR = 10;
const NUMBER_ARROW_KEYS: readonly string[] = [KEY_CODE.ARROW_UP,KEY_CODE.ARROW_DOWN];
const RANGE_KEYS: readonly string[] = [...NUMBER_ARROW_KEYS,KEY_CODE.ARROW_LEFT,KEY_CODE.ARROW_RIGHT,KEY_CODE.HOME,KEY_CODE.END,KEY_CODE.PAGE_UP,KEY_CODE.PAGE_DOWN];

/**
 * Owns property gestures, borrowing document snapshots and restoration from the tool.
 * Source-editor history remains separate. A newer unrelated property edit invalidates undo.
 */
export class NumericControls {
  declare root: HTMLElement;
  declare owner: { read?: () => string; restore?: (text: string) => void; changed?: () => void; commit?: (input: HTMLInputElement) => void; selector?: string; };
  declare gesture: { before: string; input: HTMLInputElement; drag: boolean; } | null;
  declare past: { before: string; after: string; }[];
  declare future: { before: string; after: string; }[];

  /** @param root @param signal
   * @param [owner] */
  constructor(root: HTMLElement, signal: AbortSignal, owner: {
    read?: () => string;
    restore?: (text: string) => void;
    changed?: () => void;
    commit?: (input: HTMLInputElement) => void;
    selector?: string;
} = {}) {
    this.root = root;
    this.owner = owner;

    this.gesture = null;

    this.past = [];

    this.future = [];
    root.addEventListener(BROWSER_EVENT.POINTERDOWN, event => {
      if(event.target instanceof HTMLInputElement && this.accepts(event.target) && event.target.type === 'range' && event.button === 0) {
        this.begin(event.target, true);
      }
    }, {signal, capture:true});
    root.addEventListener(BROWSER_EVENT.FOCUSIN, event => {
      if(event.target instanceof HTMLInputElement && this.accepts(event.target) && event.target.type === 'number') {
        this.begin(event.target, false);
      }
    }, {signal});
    root.addEventListener(BROWSER_EVENT.FOCUSOUT, event => {
      if(this.gesture?.drag && event.target === this.gesture.input) {
        this.cancel();
      } else if(!this.gesture?.drag) {
        this.finish();
      }
    }, {signal});
    root.addEventListener(BROWSER_EVENT.INPUT, event => {
      if(event.target instanceof HTMLInputElement && this.accepts(event.target) && event.target.type === 'number') {
        event.target.setCustomValidity('');
        this.showError(event.target, '');
      }
    }, {signal, capture:true});
    root.addEventListener(BROWSER_EVENT.INPUT, event => {
      if(event.target instanceof HTMLInputElement && this.accepts(event.target) && event.target.type === 'range') {
        this.validate(false);
      }
    }, {signal});
    root.addEventListener(BROWSER_EVENT.CHANGE, event => {
      this.validate(false);
      if(event.target instanceof HTMLInputElement && this.accepts(event.target) && event.target.type === 'number') {
        this.finish();
        this.begin(event.target, false);
      }
    }, {signal});
    document.addEventListener(BROWSER_EVENT.POINTERUP, () => {
      if(this.gesture?.drag) {
        this.finish();
      }
    }, {signal});
    document.addEventListener(BROWSER_EVENT.POINTERCANCEL, () => this.cancel(), {signal});
    root.addEventListener(BROWSER_EVENT.LOSTPOINTERCAPTURE, event => {
      if(this.gesture?.drag && event.target === this.gesture.input) {
        this.cancel();
      }
    }, {signal, capture:true});
    window.addEventListener(BROWSER_EVENT.BLUR, () => {
      if(this.gesture?.drag) {
        this.cancel();
      }
    }, {signal});
    root.addEventListener(BROWSER_EVENT.KEYDOWN, event => {
      if(event.key === KEY_CODE.ESCAPE && this.gesture?.drag) {
        event.preventDefault();
        this.cancel();
        return;
      }
      const input = event.target;
      if(input instanceof HTMLInputElement && this.accepts(input) && input.type === 'range' && RANGE_KEYS.includes(event.key) && !this.gesture) {
        this.begin(input, false);
      }
      if(!(input instanceof HTMLInputElement) || !this.accepts(input) || input.type !== 'number' || event.isComposing || event.ctrlKey || event.metaKey || (!event.shiftKey && !event.altKey && input.dataset.numericStep === undefined) || !NUMBER_ARROW_KEYS.includes(event.key)) {
        return;
      }
      if(!Number.isFinite(input.valueAsNumber)) {
        return;
      }
      event.preventDefault();
      input.dataset.numericStep ??= input.step;
      const step = Number(input.dataset.numericStep) > 0 ? Number(input.dataset.numericStep) : 0.1;
      const factor = event.shiftKey ? (input.dataset.numericInteger === 'true' ? 1 : FINE_FACTOR) : event.altKey ? COARSE_FACTOR : 1;
      const direction = event.key === KEY_CODE.ARROW_UP ? 1 : -1;
      const minimum = input.min === '' ? -Infinity : Number(input.min);
      const maximum = input.max === '' ? Infinity : Number(input.max);
      const value = Math.max(minimum, Math.min(maximum, input.valueAsNumber + direction*step*factor));
      // Fine values need not be multiples of the coarse UI step; owners validate bounds.
      input.step = 'any';
      input.value = String(Number(value.toPrecision(12)));
      input.setCustomValidity('');
      this.owner.commit?.(input);
      this.finish();
      this.begin(input, false);
    }, {signal});
    root.addEventListener(BROWSER_EVENT.KEYUP, event => {
      if(event.target instanceof HTMLInputElement && event.target.type === 'range' && this.gesture?.input === event.target) {
        this.finish();
      }
    }, {signal});
  }

  /** Refuse capture/navigation that would discard a repairable numeric draft. */
  validate(throwOnError: boolean = true) {
    if(this.root.hidden) {
      return true;
    }
    let invalid = false;
    for(const input of this.root.querySelectorAll('input[type="number"]')) {
      if(!(input instanceof HTMLInputElement) || !this.accepts(input)) {
        continue;
      }
      const valid = Number.isFinite(input.valueAsNumber) && (input.dataset.numericInteger !== 'true' || Number.isInteger(input.valueAsNumber)) && !input.validity.rangeUnderflow && !input.validity.rangeOverflow && !input.validity.customError && !input.validity.badInput;
      this.showError(input, valid ? '' : input.validationMessage || 'Enter a finite number.');
      invalid ||= !valid;
    }
    if(invalid && throwOnError) {
      throw new Error('Correct the highlighted numeric field before continuing.');
    }
    return !invalid;
  }

  /** Flush a focused valid numeric draft before explicit save or navigation. */
  capture() {
    this.validate();
    if(this.gesture?.drag) {
      return;
    }
    const input = document.activeElement;
    if(input instanceof HTMLInputElement && this.root.contains(input) && this.accepts(input) && input.type === 'number') {
      this.owner.commit?.(input);
    }
    this.finish();
    this.validate();
    if(input instanceof HTMLInputElement && input.isConnected && document.activeElement === input && this.accepts(input) && input.type === 'number') {
      this.begin(input, false);
    }
  }

  /** Commit one accepted gesture, independent of the number of input events. */
  finish() {
    const gesture = this.gesture;
    this.gesture = null;
    const after = this.owner.read?.();
    if(gesture && after !== undefined && gesture.before !== after && this.validate(false)) {
      this.past.push({before:gesture.before, after});
      this.past.splice(0, Math.max(0, this.past.length - HISTORY_LIMIT));
      this.future = [];
    }
    this.owner.changed?.();
  }

  /** Restore the pre-drag value; no authored undo entry is added. */
  cancel() {
    const gesture = this.gesture;
    this.gesture = null;
    if(gesture?.drag) {
      this.owner.restore?.(gesture.before);
    }
    this.owner.changed?.();
  }

  undo() {
    this.moveHistory(false);
  }
  redo() {
    this.moveHistory(true);
  }
  clear() {
    this.gesture = null;
    this.past = [];
    this.future = [];
    this.owner.changed?.();
  }

  /** @private @param redo */
  private moveHistory(redo: boolean) {
    this.finish();
    const from = redo ? this.future : this.past;
    const to = redo ? this.past : this.future;
    const entry = from.at(-1);
    if(!entry || !this.validate(false)) {
      return;
    }
    if(this.owner.read?.() !== (redo ? entry.before : entry.after)) {
      this.clear();
      return;
    }
    from.pop();
    this.owner.restore?.(redo ? entry.after : entry.before);
    to.push(entry);
    this.owner.changed?.();
  }

  /** @private @param input @param drag */
  private begin(input: HTMLInputElement, drag: boolean) {
    this.finish();
    const before = this.owner.read?.();
    if(before !== undefined && before.length <= MAX_SNAPSHOT_BYTES && this.validate(false)) {
      this.gesture = {before,input,drag};
    }
    input.title ||= 'Type directly · Shift + arrows fine · Alt + arrows coarse';
  }

  /** @private @param input */
  private accepts(input: HTMLInputElement) {
    return !this.owner.selector || input.matches(this.owner.selector);
  }

  /** @private @param input @param message */
  private showError(input: HTMLInputElement, message: string) {
    let output = input.dataset.numericError ? document.getElementById(input.dataset.numericError) : null;
    if(!output && message) {
      if(!input.hasAttribute('aria-label') && input.labels?.length) {
        input.setAttribute('aria-label', input.labels[0].textContent?.trim() ?? 'Value');
      }
      output = document.createElement('small');
      output.className = 'numeric-error';
      output.id = `numeric-error-${crypto.randomUUID()}`;
      input.dataset.numericError = output.id;
      output.setAttribute('role','status');
      input.parentElement?.append(output);
      const descriptions = new Set((input.getAttribute('aria-describedby') ?? '').split(' ').filter(Boolean));
      descriptions.add(output.id);
      input.setAttribute('aria-describedby', [...descriptions].join(' '));
    }
    input.setAttribute('aria-invalid', String(Boolean(message)));
    if(output instanceof HTMLElement) {
      output.textContent = message;
      output.hidden = !message;
    }
  }
}
