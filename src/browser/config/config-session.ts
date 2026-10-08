// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { ConfigField } from '../../core/config-values.ts';

import { createConfigValues } from '../../core/config-values.ts';

export type { ConfigField };
export type ConfigSessionListener = () => void;

const MAXIMUM_HISTORY_LENGTH = 100;

export interface ConfigSessionOptions {
  fields: readonly ConfigField[];
  values?: Readonly<Record<string, number>>;
  onChange?: (values: Record<string, number>) => void;
  save?: (values: Record<string, number>) => Promise<void>;
}

/**
 * Own one independent editing history and saved baseline. */
export class ConfigSession {
  declare fieldDefinitions: ConfigField[];
  declare defaultValues: Record<string, number>;
  declare currentValues: Record<string, number>;
  declare savedValues: { [x: string]: number; };
  declare onChange: (values: Record<string, number>) => void;
  declare saveValues: ((values: Record<string, number>) => Promise<void>) | undefined;
  declare history: Record<string, number>[];
  declare future: Record<string, number>[];
  declare editing: boolean;
  declare editRemembered: boolean;
  declare listeners: Set<ConfigSessionListener>;
  declare destroyed: boolean;
  declare saveCount: number;
  declare nextSaveGeneration: number;
  declare savedGeneration: number;

  /**
   * @param options
   * @param options.fields Numeric configuration contract.
   * @param [options.values] Partial initial overrides.
   * @param [options.onChange] Runtime adapter receiving complete copies.
   * @param [options.save] Persistence adapter receiving a complete snapshot.
   */
  constructor({ fields, values = {}, onChange = () => {}, save }: ConfigSessionOptions) {
    this.fieldDefinitions = fields.map(field => ({ ...field }));
    this.defaultValues = createConfigValues(this.fieldDefinitions);
    this.currentValues = createConfigValues(this.fieldDefinitions, values);
    this.savedValues = { ...this.currentValues };
    this.onChange = onChange;
    this.saveValues = save;

    this.history = [];

    this.future = [];
    this.editing = false;
    this.editRemembered = false;

    this.listeners = new Set();
    this.destroyed = false;
    this.saveCount = 0;
    this.nextSaveGeneration = 0;
    this.savedGeneration = 0;
  }

  /** @returns Copied field records. */
  get fields() {
    return this.fieldDefinitions.map(field => ({ ...field }));
  }

  /** @returns Copied complete values. */
  get values() {
    return { ...this.currentValues };
  }

  /** @returns Copied values from the latest successful save. */
  get storedValues() {
    return { ...this.savedValues };
  }

  /**
   * Whether current values differ from the latest saved baseline. */
  get dirty() {
    return !recordsEqual(this.currentValues, this.savedValues);
  }

  /**
   * Whether an earlier accepted edit remains in history. */
  get canUndo() {
    return this.history.length > 0;
  }

  /**
   * Whether an undone edit is available to restore. */
  get canRedo() {
    return this.future.length > 0;
  }

  /**
   * Whether this session has an explicit persistence adapter. */
  get canSave() {
    return Boolean(this.saveValues);
  }

  /**
   * Begin a continuous edit; repeated calls retain its original undo baseline. */
  beginEdit() {
    this.assertActive();
    if(!this.editing) {
      this.editing = true;
      this.editRemembered = false;
    }
  }

  /**
   * Finish a continuous edit. Accepted previews remain live and undo together. */
  endEdit() {
    this.assertActive();
    this.editing = false;
    this.editRemembered = false;
  }

  /**
   * Apply a validated partial preset as one undoable edit.
   * @param values
   */
  setValues(values: Readonly<Record<string, number>>) {
    this.assertActive();
    const validated = createConfigValues(this.fieldDefinitions, {...this.currentValues, ...values});
    this.endEdit();
    this.applyValues(validated, true);
  }

  /**
   * Whether any persistence request is still pending. */
  get saving() {
    return this.saveCount > 0;
  }

  /**
   * Validate and apply one field; rejected values leave current state unchanged.
   * @param key
   * @param value
   */
  setValue(key: string, value: number) {
    this.assertActive();
    const nextValues = { ...this.currentValues, [key]: value };
    const validatedValues = createConfigValues(this.fieldDefinitions, nextValues);
    this.applyValues(validatedValues, true);
  }

  /**
   * Restore the last accepted edit and notify observers after history changes. */
  undo() {
    this.assertActive();
    this.endEdit();
    const previousValues = this.history.at(-1);
    if(!previousValues) {
      return;
    }
    const current = {...this.currentValues};
    this.applyValues(previousValues, false, false);
    this.future.push(current);
    this.history.pop();
    this.notify();
  }

  /**
   * Restore the latest undone edit; a rejected runtime transition retains history. */
  redo() {
    this.assertActive();
    this.endEdit();
    const next = this.future.at(-1);
    if(!next) {
      return;
    }
    const current = {...this.currentValues};
    this.applyValues(next, false, false);
    this.history.push(current);
    this.future.pop();
    this.notify();
  }

  /**
   * Restore the saved baseline as an undoable edit. */
  revert() {
    this.assertActive();
    this.endEdit();
    this.applyValues(this.savedValues, true);
  }

  /**
   * Restore one authored default as an undoable edit.
   * @param key
   */
  resetField(key: string) {
    this.assertActive();
    if(!Object.hasOwn(this.defaultValues, key)) {
      throw new RangeError(`Unknown config key "${key}".`);
    }
    this.endEdit();
    this.setValue(key, this.defaultValues[key]);
  }

  /**
   * Restore all authored defaults in one undo transaction. */
  resetAll() {
    this.assertActive();
    this.endEdit();
    this.applyValues(this.defaultValues, true);
  }

  /**
   * Persist a copied snapshot; newer edits can continue while the request runs.
   * Out-of-order completions cannot replace a newer saved baseline.
   * Rejects when no persistence adapter exists or that adapter fails.
   */
  async save() {
    this.assertActive();
    if(!this.saveValues) {
      throw new Error('This config session has no save adapter.');
    }
    const snapshot = { ...this.currentValues };
    const generation = ++this.nextSaveGeneration;
    this.saveCount++;
    this.notify();
    try {
      await this.saveValues({ ...snapshot });
      if(generation > this.savedGeneration) {
        this.savedValues = snapshot;
        this.savedGeneration = generation;
      }
    } finally {
      this.saveCount--;
      this.notify();
    }
  }

  /**
   * Subscribe to state changes until the returned unsubscribe function is called.
   * @param listener Borrowed synchronous observer.
   * @returns Idempotent unsubscribe function.
   */
  subscribe(listener: ConfigSessionListener): () => void {
    this.assertActive();
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  /**
   * Release observers and history; pending saves retain their own snapshots. */
  destroy() {
    if(this.destroyed) {
      return;
    }
    this.listeners.clear();
    this.history.length = 0;
    this.future.length = 0;
    this.destroyed = true;
  }

  /**
   * Commit copied values only after the runtime accepts them.
   * @private
   * @param values
   * @param remember Whether to retain the previous values for undo.
   * @param [notify] Whether observers run before this call returns.
   */
  private applyValues(values: Record<string, number>, remember: boolean, notify: boolean = true) {
    if(recordsEqual(this.currentValues, values)) {
      return;
    }
    // The runtime is allowed to reject a transition. Commit owned state only
    // after the adapter accepts the complete copied record.
    this.onChange({ ...values });
    if(remember) {
      this.future.length = 0;
    }
    if(remember && (!this.editing || !this.editRemembered)) {
      this.editRemembered = this.editing;
      this.history.push({ ...this.currentValues });
      if(this.history.length > MAXIMUM_HISTORY_LENGTH) {
        this.history.shift();
      }
    }
    this.currentValues = { ...values };
    if(notify) {
      this.notify();
    }
  }

  /**
   * Notify current observers synchronously in registration order. @private */
  private notify() {
    for(const listener of this.listeners) {
      listener();
    }
  }

  /**
   * Reject mutations and subscriptions after disposal. @private */
  private assertActive() {
    if(this.destroyed) {
      throw new Error('Config session has been destroyed.');
    }
  }
}

/**
 * Compare complete flat numeric records without retaining either input.
 * @param left
 * @param right
 */
function recordsEqual(left: Record<string, number>, right: Record<string, number>) {
  const keys = Object.keys(left);
  return keys.length === Object.keys(right).length && keys.every(key => left[key] === right[key]);
}
