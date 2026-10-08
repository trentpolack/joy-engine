// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT, PRIMITIVE_SHAPE, TRANSFORM_PROPERTY } from 'joy-engine/constants';
import type { JsonValue, JsonRecord } from 'joy-engine';

export type { JsonValue };
export interface Asset {
  path: string;
  kind: string;
}
export interface FieldMetadata {
  label?: string;
  unit?: string;
  min?: number;
  max?: number;
  step?: number;
  choices?: string[];
}
const UNITS = {mass: 'kg', radius: 'm', halfExtents: 'm', size: 'm', position: 'm', lifetime: 's', duration: 's', range: 'm', speed: 'm/s'};

/**
 * Render nested component data as ordinary controls. The document owner commits history.
 * Unknown fields remain editable; metadata is optional and never required for round trips.
 */
export class PropertyEditor {
  declare root: HTMLElement;
  declare actions: { change: (path: string[], value: JsonValue | undefined) => void; error: (message: string) => void; open: (path: string) => void; assets: () => Asset[]; read: (path: string[]) => JsonValue | undefined; metadata?: Record<string, FieldMetadata>; };
  declare rows: number;
  declare pending: Map<HTMLInputElement, () => void>;
  declare commits: WeakMap<HTMLInputElement, () => void>;
  declare groups: Set<HTMLDetailsElement>;
  declare closed: Set<string>;

  /** @param root
   * @param actions
   */
  constructor(root: HTMLElement, actions: {
    change: (path: string[], value: JsonValue | undefined) => void;
    error: (message: string) => void;
    open: (path: string) => void;
    assets: () => Asset[];
    read: (path: string[]) => JsonValue | undefined;
    metadata?: Record<string, FieldMetadata>;
}) {
    this.root = root;
    this.actions = actions;
    this.rows = 0;

    this.pending = new Map();

    this.commits = new WeakMap();

    this.groups = new Set();

    this.closed = new Set();
  }

  /**
   * Replace inspector controls from a copied, validated component record.
   * @param components
   */
  render(components: Record<string, JsonRecord>) {
    for(const group of this.groups) {
      const key = group.dataset.path ?? '';
      if(group.open) {
        this.closed.delete(key);
      } else { this.closed.add(key); }
    }
    this.groups.clear();
    this.pending.clear();
    this.rows = 0;
    this.root.replaceChildren();
    for(const [name, value] of Object.entries(components)) {
      this.field(this.root, [name], value);
    }
  }

  /**
   * Commit pending valid controls before save/navigation; invalid text stays visible. */
  capture() {
    for(const [input, commit] of [...this.pending]) {
      if(input.isConnected) {
        commit();
      }
    }
    const invalid = this.root.querySelector(':invalid');
    if(invalid) {
      throw new Error('Correct the highlighted property before saving.');
    }
  }

  /** Commit a keyboard adjustment through the existing document owner. @param input */
  commitInput(input: HTMLInputElement) {
    this.commits.get(input)?.();
  }

  /**
   * Render one bounded field or nested group. @private
   * @param parent @param path @param value
   */
  private field(parent: HTMLElement, path: string[], value: JsonValue) {
    if(++this.rows > 600 || path.length > 12) {
      const note = document.createElement('p');
      note.textContent = 'Large data is preserved. Use Advanced Source for this section.';
      parent.append(note);
      return;
    }
    const key = path.join('.');
    const name = path.at(-1) ?? '';
    if(Array.isArray(value) && [3, 4].includes(value.length) && value.every(item => typeof item === 'number') && (['size', 'halfExtents', 'color', TRANSFORM_PROPERTY.POSITION, TRANSFORM_PROPERTY.ROTATION, TRANSFORM_PROPERTY.SCALE] as readonly string[]).includes(name)) {
      this.vector(parent, path,  ((value) as number[]));
      return;
    }
    if(value !== null && typeof value === 'object') {
      const group = document.createElement('details');
      group.className = 'property-group';
      group.dataset.path = key;
      group.open = !this.closed.has(key) && path.length < 3;
      this.groups.add(group);
      const summary = document.createElement('summary');
      summary.textContent = `${name.replaceAll('_', ' ')}${Array.isArray(value) ? ` · ${value.length}` : ''}`;
      group.append(summary);
      const contents = document.createElement('div');
      contents.className = 'property-contents';
      for(const [child, entry] of Object.entries(value)) {
        this.field(contents, [...path, child], entry);
      }
      contents.append(this.addField(path, value), this.removeButton(path));
      group.append(contents);
      parent.append(group);
      return;
    }
    const row = document.createElement('div');
    row.className = 'property-row';
    const metadata = this.actions.metadata?.[key] ?? {};
    const unit = metadata.unit ?? UNITS[ ((name) as keyof typeof UNITS)];
    const label = document.createElement('label');
    label.textContent = `${metadata.label ?? name.replaceAll('_', ' ')}${unit ? ` · ${unit}` : ''}`;
    const id = `property-${key}`;
    label.htmlFor = id;
    row.append(label);
    if(name === 'asset' && (path[0] === 'render' || path[0] === 'effect')) {
      const picker = document.createElement('select');
      picker.id = id;
      picker.setAttribute('aria-label', key);
      picker.append(option('', 'Choose an asset…'));
      const assets = this.actions.assets().filter(asset => path[0] === 'effect' ? asset.kind === 'joyfx' : ['form', 'formlab'].includes(asset.kind));
      if(typeof value === 'string' && value && !assets.some(asset => asset.path === value)) {
        picker.append(option(value, `${value} (missing)`));
      }
      for(const asset of assets) { picker.append(option(asset.path, asset.path.replace('assets/', ''))); }
      picker.value = String(value ?? '');
      picker.addEventListener(BROWSER_EVENT.CHANGE, () => this.commit(path, picker.value));
      const edit = document.createElement('button');
      edit.type = 'button';
      edit.textContent = 'EDIT SOURCE ↗';
      edit.disabled = !value;
      edit.addEventListener(BROWSER_EVENT.CLICK, () => this.actions.open(String(value)));
      row.append(picker, edit);
    } else if(metadata.choices || (name === 'shape' && ['render', 'physics'].includes(path[0]))) {
      const select = document.createElement('select');
      select.id = id;
      select.setAttribute('aria-label', key);
      const choices = metadata.choices ?? (path[0] === 'physics' ? [PRIMITIVE_SHAPE.BOX, PRIMITIVE_SHAPE.SPHERE, PRIMITIVE_SHAPE.PLANE] : [PRIMITIVE_SHAPE.BOX, PRIMITIVE_SHAPE.SPHERE]);
      for(const choice of new Set([String(value), ...choices])) { select.append(option(choice, choice)); }
      select.value = String(value);
      select.addEventListener(BROWSER_EVENT.CHANGE, () => this.commit(path, select.value));
      row.append(select);
    } else {
      const input = document.createElement('input');
      input.id = id;
      input.setAttribute('aria-label', key);
      input.type = typeof value === 'boolean' ? 'checkbox' : typeof value === 'number' ? 'number' : 'text';
      if(typeof value === 'number') {
        input.step = String(metadata.step ?? 'any');
        if(metadata.min !== undefined) {
          input.min = String(metadata.min);
        }
        if(metadata.max !== undefined) {
          input.max = String(metadata.max);
        }
      }
      input.value = value === null ? '' : String(value);
      input.checked = value === true;
      if(value === null) {
        input.placeholder = 'null — enter text to replace';
      }
      const commit = () => {
        input.setCustomValidity('');
        const next = input.type === 'checkbox' ? input.checked : input.type === 'number' ? input.valueAsNumber : input.value;
        if((typeof next === 'number' && !Number.isFinite(next)) || !input.checkValidity()) {
          input.setCustomValidity('Enter a valid value.');
          throw new Error(`${key}: enter a valid value.`);
        }
        this.pending.delete(input);
        this.actions.change(path, next);
      };
      this.commits.set(input, commit);
      input.addEventListener(BROWSER_EVENT.INPUT, () => { input.setCustomValidity(''); this.pending.set(input, commit); });
      input.addEventListener(BROWSER_EVENT.CHANGE, () => {
        try { commit(); } catch(error) { this.actions.error(error instanceof Error ? error.message : String(error)); }
      });
      row.append(input);
    }
    row.append(this.removeButton(path));
    parent.append(row);
  }

  /**
   * Keep spatial and color triples compact and dimensionally valid.
   * @private @param parent @param path @param values
   */
  private vector(parent: HTMLElement, path: string[], values: number[]) {
    const group = document.createElement('fieldset');
    group.className = 'property-vector';
    const legend = document.createElement('legend');
    legend.textContent = path.at(-1)?.toUpperCase() ?? '';
    group.append(legend);
    for(let index = 0; index < values.length; index++) {
      const label = document.createElement('label');
      label.textContent = (path.at(-1) === 'color' ? ['R', 'G', 'B', 'A'] : ['X', 'Y', 'Z', 'W'])[index];
      const input = document.createElement('input');
      input.type = 'number'; input.step = 'any'; input.value = String(values[index]);
      input.setAttribute('aria-label', `${path.join('.')}.${index}`);
      const commit = () => {
        input.setCustomValidity('');
        if(!Number.isFinite(input.valueAsNumber)) {
          input.setCustomValidity('Enter a finite number.'); throw new Error('Enter a finite vector component.');
        }
        const next = [...values]; next[index] = input.valueAsNumber;
        this.actions.change(path, next);
        values[index] = next[index];
        this.pending.delete(input);
      };
      this.commits.set(input, commit);
      input.addEventListener(BROWSER_EVENT.INPUT, () => { input.setCustomValidity(''); this.pending.set(input, commit); });
      input.addEventListener(BROWSER_EVENT.CHANGE, () => { try { commit(); } catch(error) { this.actions.error(String(error)); } });
      label.append(input); group.append(label);
    }
    parent.append(group);
  }

  /**
   * Create fields without source editing; arrays duplicate the last item as a useful starter.
   * @private @param path @param value
   */
  private addField(path: string[], value: JsonValue[] | Record<string, JsonValue>) {
    const form = document.createElement('div');
    form.className = 'property-add';
    const name = document.createElement('input');
    name.placeholder = 'New property';
    name.setAttribute('aria-label', `New property in ${path.join('.')}`);
    const type = document.createElement('select');
    type.setAttribute('aria-label', 'Property type');
    for(const text of ['number', 'text', 'boolean', 'group', 'array']) { type.append(option(text, text)); }
    const add = document.createElement('button');
    add.textContent = Array.isArray(value) ? '+ ITEM' : '+ ADD';
    add.addEventListener(BROWSER_EVENT.CLICK, () => {
      try {
        this.capture();
      } catch(error) {
        this.actions.error(error instanceof Error ? error.message : String(error));
        return;
      }
      const next = structuredClone(this.actions.read(path) ?? value);
      if(next === null || typeof next !== 'object') {
        return;
      }
      if(Array.isArray(next)) {
        next.push(next.length ? structuredClone(next.at(-1) ?? null) : 0);
      } else {
        const key = name.value.trim();
        if(!key || Object.hasOwn(next, key)) {
          this.actions.error('Use a unique property name.'); return;
        }
        const initial = type.value === 'number' ? 0 : type.value === 'boolean' ? false : type.value === 'group' ? {} : type.value === 'array' ? [] : '';
        Object.defineProperty(next, key, {value: initial, enumerable: true, writable: true, configurable: true});
      }
      this.commit(path, next);
    });
    if(!Array.isArray(value)) {
      form.append(name, type);
    }
    form.append(add);
    return form;
  }

  /** @private @param path */
  private removeButton(path: string[]) {
    const button = document.createElement('button');
    button.className = 'remove-property';
    button.textContent = '−';
    button.title = `Remove ${path.join('.')}`;
    button.setAttribute('aria-label', button.title);
    button.addEventListener(BROWSER_EVENT.CLICK, () => this.commit(path, undefined));
    return button;
  }

  /** @private @param path @param value */
  private commit(path: string[], value: JsonValue | undefined) {
    try { this.capture(); this.actions.change(path, value); } catch(error) { this.actions.error(error instanceof Error ? error.message : String(error)); }
  }
}

/** @param value @param text */
function option(value: string, text: string) {
  const node = document.createElement('option');
  node.value = value;
  node.textContent = text;
  return node;
}
