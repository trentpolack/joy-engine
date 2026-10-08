// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { TUNING_APPLY_MODE } from './constants.ts';
import type { ConfigField } from '../../core/config-values.ts';
import type { ConfigPanel } from '../config/config-panel.ts';
import { ConfigSession } from '../config/config-session.ts';
import { createConfigValues } from '../../core/config-values.ts';

export type TuningField = Omit<ConfigField, 'defaultValue' | 'applyMode' | 'group'> & {
    defaultValue?: number;
    group?: string;
};
export interface TuningBinding {
  target: object;
  property: string;
  field: ConfigField;
}

export interface LiveTuningOptions {
  title?: string;
  save?: (values: Record<string, number>) => Promise<void>;
}

/**
 * Own shared history and an optional panel for borrowed live numeric objects. */
export class LiveTuning {
  declare title: string;
  declare save: ((values: Record<string, number>) => Promise<void>) | undefined;
  declare bindings: Map<string, TuningBinding>;
  declare identifiers: Set<string>;
  declare editSession: ConfigSession | null;
  declare panel: ConfigPanel | null;
  declare mounting: Promise<void> | null;
  declare destroyed: boolean;

  /**
   * @param [options]
   * @param [options.title]
   * @param [options.save] Explicit persistence; omitted means session-only.
   */
  constructor({title = 'LIVE TUNING', save}: LiveTuningOptions = {}) {
    this.title = title;
    this.save = save;

    this.bindings = new Map();
    this.identifiers = new Set();

    this.editSession = null;

    this.panel = null;

    this.mounting = null;
    this.destroyed = false;
  }

  /**
   * Register writable numeric data properties before accessing session or mounting.
   * Defaults capture current values unless metadata provides authored defaults.
   * Keys are namespaced as id.property; unrelated object properties are untouched.
   * @param id Stable object identifier without dots.
   * @param target Borrowed object; no setters, proxies or property replacement while registered.
   * @param fields Explicit numeric metadata; never inferred by scanning objects.
   */
  register(id: string, target: object, fields: readonly TuningField[]) {
    if(this.destroyed || this.editSession) {
      throw new Error('Register live objects before opening the tuning session.');
    }
    if(!/^[A-Za-z][\w-]*$/.test(id)) {
      throw new RangeError('Tuning identifiers must start with a letter and contain no dots.');
    }
    if(this.identifiers.has(id)) {
      throw new RangeError(`Duplicate tuning identifier "${id}".`);
    }
    const additions = fields.map(field => {
      const descriptor = Object.getOwnPropertyDescriptor(target, field.key);
      if(!descriptor || !descriptor.writable || typeof descriptor.value !== 'number') {
        throw new TypeError(`Tuning property "${field.key}" must be a writable numeric data property.`);
      }
      return {target, property: field.key, field: {
        ...field, key: `${id}.${field.key}`, group: field.group ?? id,
        defaultValue: field.defaultValue ?? descriptor.value, applyMode:  (TUNING_APPLY_MODE.LIVE)
      }};
    });
    const definitions = additions.map(binding => binding.field);
    createConfigValues(definitions, Object.fromEntries(additions.map(binding => [binding.field.key, Reflect.get(target, binding.property)])));
    this.identifiers.add(id);
    for(const binding of additions) {
      this.bindings.set(binding.field.key, binding);
    }
  }

  /**
   * Lazily finalize registration and return the owned editing session. */
  get session() {
    if(!this.editSession) {
      if(this.destroyed) {
        throw new Error('Live tuning has been destroyed.');
      }
      const bindings = [...this.bindings.values()];
      this.editSession = new ConfigSession({
        fields: bindings.map(binding => binding.field),
        values: Object.fromEntries(bindings.map(binding => [binding.field.key, Reflect.get(binding.target, binding.property)])),
        save: this.save,
        onChange: values => {
          // Validate every borrowed descriptor before writing any property. Owners
          // must not replace properties with setters during this registration.
          for(const binding of bindings) {
            const descriptor = Object.getOwnPropertyDescriptor(binding.target, binding.property);
            if(!descriptor?.writable || !Object.hasOwn(descriptor, 'value')) {
              throw new TypeError(`Tuning property "${binding.field.key}" is no longer writable.`);
            }
          }
          for(const binding of bindings) {
            Reflect.set(binding.target, binding.property, values[binding.field.key]);
          }
        }
      });
    }
    return this.editSession;
  }

  /**
   * Apply a copied partial preset through validation and one shared undo transaction.
   * @param values Namespaced numeric values.
   */
  applyPreset(values: Readonly<Record<string, number>>) {
    this.session.setValues(values);
  }

  /**
   * Return a copied session preset; persistence is always an explicit caller operation. */
  capturePreset() {
    return this.session.values;
  }

  /**
   * Mount the shared panel once; destruction during loading prevents stale DOM creation. */
  async mount() {
    if(this.destroyed) {
      throw new Error('Live tuning has been destroyed.');
    }
    if(!this.mounting) {
      const session = this.session;
      this.mounting = import('../config/config-panel.ts').then(({ConfigPanel}) => {
        if(!this.destroyed) {
          this.panel = new ConfigPanel({title: this.title, session});
        }
      });
    }
    await this.mounting;
  }

  /**
   * Remove owned UI/history; current live values stay on their borrowed owners. */
  destroy() {
    if(this.destroyed) {
      return;
    }
    this.panel?.destroy();
    this.editSession?.destroy();
    this.bindings.clear();
    this.destroyed = true;
  }
}
