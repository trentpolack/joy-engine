// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { BROWSER_EVENT, COMPONENT_FIELD_KIND, TRANSFORM_PROPERTY } from 'joy-engine/constants';
import { isComponentFieldActive } from 'joy-engine';
import type { ComponentFieldSchema, ComponentIssue, ComponentSchemaCatalog, JsonRecord, JsonValue } from 'joy-engine';

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
export interface PropertyEditorActions {
  change: (path: string[], value: JsonValue | undefined) => void;
  error: (message: string) => void;
  open: (path: string) => void;
  assets: () => Asset[];
  read: (path: string[]) => JsonValue | undefined;
  /** Borrowed component schemas; custom components without a schema stay generically editable. */
  schemas: ComponentSchemaCatalog;
  /** Per-path overrides for asset-declared parameters (FORM/JOYFX bounds), keyed by dotted path. */
  metadata?: Record<string, FieldMetadata>;
}

// Inspector bounds; larger data stays intact and remains editable through Advanced Source.
const MAX_ROWS = 600;
const MAX_DEPTH = 12;

// Custom components have no schema, so their controls fall back to these name conventions.
const CUSTOM_UNITS = {mass: 'kg', radius: 'm', halfExtents: 'm', size: 'm', position: 'm', lifetime: 's', duration: 's', range: 'm', speed: 'm/s', intensity: 'linear'};
const CUSTOM_VECTOR_NAMES: readonly string[] = ['size', 'halfExtents', 'color', TRANSFORM_PROPERTY.POSITION, TRANSFORM_PROPERTY.ROTATION, TRANSFORM_PROPERTY.SCALE];

/**
 * Render nested component data as ordinary controls. The document owner commits history.
 * Component schemas choose controls, units, bounds, field order and missing-field suggestions;
 * fields outside a schema remain editable and metadata is never required for round trips.
 */
export class PropertyEditor {
  declare root: HTMLElement;
  declare actions: PropertyEditorActions;
  declare rows: number;
  declare pending: Map<HTMLInputElement, () => void>;
  declare commits: WeakMap<HTMLInputElement, () => void>;
  declare groups: Set<HTMLDetailsElement>;
  declare closed: Set<string>;
  declare issues: readonly ComponentIssue[];

  /**
   * @param root Owned container whose children this editor replaces on every render.
   * @param actions Borrowed document callbacks and schema catalog.
   */
  constructor(root: HTMLElement, actions: PropertyEditorActions) {
    this.root = root;
    this.actions = actions;
    this.rows = 0;

    // Inputs whose typed value has not been committed yet, and how to commit each one.
    this.pending = new Map();
    this.commits = new WeakMap();

    // Group open/closed state survives re-renders by dotted path.
    this.groups = new Set();
    this.closed = new Set();

    // Advisory issues for the rendered components; they never block editing.
    this.issues = [];
  }

  /**
   * Replace inspector controls from a copied, validated component record.
   * @param components Copied resolved components of the selected entity.
   * @param issues Schema issues for those components, shown inline.
   */
  render(components: Record<string, JsonRecord>, issues: readonly ComponentIssue[] = []) {
    for(const group of this.groups) {
      const key = group.dataset.path ?? '';
      if(group.open) {
        this.closed.delete(key);
      } else {
        this.closed.add(key);
      }
    }
    this.groups.clear();
    this.pending.clear();
    this.rows = 0;
    this.issues = issues;

    this.root.replaceChildren();
    for(const [name, value] of Object.entries(components)) {
      this.field(this.root, [name], value, null);
    }
  }

  /**
   * Refresh issue lists and field flags in place, keeping focus and pending typing intact.
   * Use after scalar edits that do not rebuild the inspector.
   * @param issues Schema issues for the currently rendered components.
   */
  showIssues(issues: readonly ComponentIssue[]) {
    this.issues = issues;
    for(const group of this.root.querySelectorAll<HTMLElement>('.component-group')) {
      const contents = group.querySelector<HTMLElement>(':scope > .property-contents');
      if(!contents) {
        continue;
      }
      contents.querySelector(':scope > .property-issues')?.remove();
      this.appendIssues(contents, group.dataset.path ?? '');
    }
    for(const element of this.root.querySelectorAll<HTMLElement>('[data-field-path]')) {
      const path = (element.dataset.fieldPath ?? '').split('.');
      this.markIssue(element, path);
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
   * @param parent Element that receives the control.
   * @param path Component type followed by the field path.
   * @param value Copied authored value.
   * @param container Record that holds this field, for sibling conditions; null for components.
   */
  private field(parent: HTMLElement, path: string[], value: JsonValue, container: JsonRecord | null) {
    if(++this.rows > MAX_ROWS || path.length > MAX_DEPTH) {
      const note = document.createElement('p');
      note.textContent = 'Large data is preserved. Use Advanced Source for this section.';
      parent.append(note);
      return;
    }

    const schema = this.fieldSchema(path);
    if(isVectorValue(schema, path, value)) {
      this.vector(parent, path, ((value) as number[]), schema, container);
      return;
    }
    if(value !== null && typeof value === 'object') {
      this.group(parent, path, value);
      return;
    }
    this.row(parent, path, value, schema, container);
  }

  /**
   * Render a component, record or list as a collapsible group. Schema fields come first in
   * schema order; custom fields follow in authored order. @private
   * @param parent Element that receives the group.
   * @param path Path of the record or list.
   * @param value Copied record or list.
   */
  private group(parent: HTMLElement, path: string[], value: JsonValue[] | JsonRecord) {
    const key = path.join('.');
    const name = path.at(-1) ?? '';
    const isComponent = path.length === 1;
    const componentSchema = isComponent ? this.actions.schemas.get(name) : null;
    const listField = this.fieldSchema(path);

    const group = document.createElement('details');
    group.className = isComponent ? 'property-group component-group' : 'property-group';
    group.dataset.path = key;
    group.open = !this.closed.has(key) && path.length < 3;
    this.groups.add(group);

    const summary = document.createElement('summary');
    summary.textContent = `${groupLabel(path, componentSchema?.label ?? listField?.label)}${Array.isArray(value) ? ` · ${value.length}` : ''}`;
    if(componentSchema && componentSchema.label.toLowerCase() !== name.toLowerCase()) {
      const typeName = document.createElement('small');
      typeName.textContent = name;
      summary.append(' ', typeName);
    }
    group.append(summary);

    const contents = document.createElement('div');
    contents.className = 'property-contents';
    if(componentSchema) {
      const description = document.createElement('p');
      description.className = 'component-description';
      description.textContent = componentSchema.description;
      contents.append(description);
    }
    if(isComponent) {
      this.appendIssues(contents, name);
    }

    // Arrays and custom records keep authored order; schema-described records follow the schema.
    const fields = Array.isArray(value) ? null : this.recordFields(path);
    for(const child of orderedKeys(value, fields)) {
      const entry = Array.isArray(value) ? value[Number(child)] : value[child];
      this.field(contents, [...path, child], entry, Array.isArray(value) ? null : value);
    }

    if(fields && !Array.isArray(value)) {
      const suggestions = this.schemaFieldAdder(path, fields, value);
      if(suggestions) {
        contents.append(suggestions);
      }
    }
    contents.append(this.addField(path, value), this.removeButton(path));
    group.append(contents);
    parent.append(group);
  }

  /**
   * Render one scalar field with the control its schema, metadata or value type implies. @private
   * @param parent Element that receives the row.
   * @param path Full field path.
   * @param value Copied scalar value.
   * @param schema Field schema, or null for custom data.
   * @param container Record that holds this field.
   */
  private row(parent: HTMLElement, path: string[], value: JsonValue, schema: ComponentFieldSchema | null, container: JsonRecord | null) {
    const key = path.join('.');
    const name = path.at(-1) ?? '';
    const metadata = this.actions.metadata?.[key] ?? {};
    const unit = metadata.unit ?? schema?.unit ?? (schema ? undefined : CUSTOM_UNITS[ ((name) as keyof typeof CUSTOM_UNITS)]);
    const id = `property-${key}`;

    const row = document.createElement('div');
    row.className = 'property-row';
    this.markState(row, path, schema, container);

    const label = document.createElement('label');
    label.textContent = `${metadata.label ?? schema?.label ?? name.replaceAll('_', ' ')}${unit ? ` · ${unit}` : ''}`;
    label.htmlFor = id;
    if(schema?.description) {
      label.title = schema.description;
    }
    row.append(label);

    if(schema?.kind === COMPONENT_FIELD_KIND.ASSET) {
      row.append(...this.assetPicker(id, key, path, value, schema.assetKinds ?? []));
    } else if(metadata.choices || schema?.choices) {
      row.append(this.choicePicker(id, key, path, value, metadata.choices ?? schema?.choices ?? []));
    } else {
      row.append(this.input(id, key, path, value, schema, metadata));
    }

    row.append(this.removeButton(path));
    parent.append(row);
  }

  /**
   * Create an asset selector limited to the field's asset kinds, keeping a missing reference visible.
   * @private
   * @returns The selector and its EDIT SOURCE button.
   */
  private assetPicker(id: string, key: string, path: string[], value: JsonValue, kinds: readonly string[]): HTMLElement[] {
    const picker = document.createElement('select');
    picker.id = id;
    picker.setAttribute('aria-label', key);
    picker.append(option('', 'Choose an asset…'));

    const assets = this.actions.assets().filter(asset => kinds.includes(asset.kind));
    if(typeof value === 'string' && value && !assets.some(asset => asset.path === value)) {
      picker.append(option(value, `${value} (missing)`));
    }
    for(const asset of assets) {
      picker.append(option(asset.path, asset.path.replace('assets/', '')));
    }
    picker.value = String(value ?? '');
    picker.addEventListener(BROWSER_EVENT.CHANGE, () => this.commit(path, picker.value));

    const edit = document.createElement('button');
    edit.type = 'button';
    edit.textContent = 'EDIT SOURCE ↗';
    edit.disabled = !value;
    edit.addEventListener(BROWSER_EVENT.CLICK, () => this.actions.open(String(value)));
    return [picker, edit];
  }

  /**
   * Create a selector over fixed choices; an unexpected authored value stays selectable. @private
   */
  private choicePicker(id: string, key: string, path: string[], value: JsonValue, choices: readonly string[]): HTMLSelectElement {
    const select = document.createElement('select');
    select.id = id;
    select.setAttribute('aria-label', key);
    for(const choice of new Set([String(value), ...choices])) {
      select.append(option(choice, choice));
    }
    select.value = String(value);
    select.addEventListener(BROWSER_EVENT.CHANGE, () => this.commit(path, select.value));
    return select;
  }

  /**
   * Create a text, number or checkbox input that commits on change and tracks pending typing. @private
   */
  private input(id: string, key: string, path: string[], value: JsonValue, schema: ComponentFieldSchema | null, metadata: FieldMetadata): HTMLInputElement {
    const input = document.createElement('input');
    input.id = id;
    input.setAttribute('aria-label', key);
    input.type = typeof value === 'boolean' ? 'checkbox' : typeof value === 'number' ? 'number' : 'text';

    if(typeof value === 'number') {
      const step = metadata.step ?? (schema?.integer ? 1 : schema?.step);
      const minimum = metadata.min ?? schema?.min;
      const maximum = metadata.max ?? schema?.max;
      input.step = String(step ?? 'any');
      if(minimum !== undefined) {
        input.min = String(minimum);
      }
      if(maximum !== undefined) {
        input.max = String(maximum);
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

    input.addEventListener(BROWSER_EVENT.INPUT, () => {
      input.setCustomValidity('');
      this.pending.set(input, commit);
    });
    input.addEventListener(BROWSER_EVENT.CHANGE, () => {
      try {
        commit();
      } catch(error) {
        this.actions.error(error instanceof Error ? error.message : String(error));
      }
    });
    return input;
  }

  /**
   * Keep spatial and color triples compact and dimensionally valid.
   * @private
   * @param parent Element that receives the vector.
   * @param path Full field path.
   * @param values Copied channel values; updated after each committed channel.
   * @param schema Field schema, or null for custom data.
   * @param container Record that holds this field.
   */
  private vector(parent: HTMLElement, path: string[], values: number[], schema: ComponentFieldSchema | null, container: JsonRecord | null) {
    const name = path.at(-1) ?? '';
    const isColor = schema ? schema.kind === COMPONENT_FIELD_KIND.COLOR : name === 'color';
    const unit = schema?.unit ?? (schema ? undefined : CUSTOM_UNITS[ ((name) as keyof typeof CUSTOM_UNITS)]);

    const group = document.createElement('fieldset');
    group.className = 'property-vector';
    this.markState(group, path, schema, container);

    const legend = document.createElement('legend');
    legend.textContent = `${schema?.label ?? name}${unit ? ` · ${unit}` : ''}`.toUpperCase();
    if(schema?.description) {
      legend.title = schema.description;
    }
    group.append(legend);

    for(let index = 0; index < values.length; index++) {
      const label = document.createElement('label');
      label.textContent = (isColor ? ['R', 'G', 'B', 'A'] : ['X', 'Y', 'Z', 'W'])[index];

      const input = document.createElement('input');
      input.type = 'number';
      input.step = String(schema?.step ?? 'any');
      if(schema?.min !== undefined) {
        input.min = String(schema.min);
      }
      if(schema?.max !== undefined) {
        input.max = String(schema.max);
      }
      input.value = String(values[index]);
      input.setAttribute('aria-label', `${path.join('.')}.${index}`);

      const commit = () => {
        input.setCustomValidity('');
        if(!Number.isFinite(input.valueAsNumber)) {
          input.setCustomValidity('Enter a finite number.');
          throw new Error('Enter a finite vector component.');
        }
        const next = [...values];
        next[index] = input.valueAsNumber;
        this.actions.change(path, next);
        values[index] = next[index];
        this.pending.delete(input);
      };
      this.commits.set(input, commit);

      input.addEventListener(BROWSER_EVENT.INPUT, () => {
        input.setCustomValidity('');
        this.pending.set(input, commit);
      });
      input.addEventListener(BROWSER_EVENT.CHANGE, () => {
        try {
          commit();
        } catch(error) {
          this.actions.error(String(error));
        }
      });
      label.append(input);
      group.append(label);
    }
    parent.append(group);
  }

  /**
   * Offer schema fields the record does not author yet. Adding one writes its runtime default, so the
   * visible value never changes behavior until the author edits it. @private
   * @returns The chooser, or null when every applicable field is present.
   */
  private schemaFieldAdder(path: string[], fields: readonly ComponentFieldSchema[], record: JsonRecord): HTMLElement | null {
    const missing = fields.filter(field => !Object.hasOwn(record, field.name) && isComponentFieldActive(field, record));
    if(missing.length === 0) {
      return null;
    }

    const form = document.createElement('div');
    form.className = 'property-add schema-add';

    const select = document.createElement('select');
    select.setAttribute('aria-label', `Add a ${path.join('.')} field`);
    for(const field of missing) {
      select.append(option(field.name, field.label ?? field.name));
    }

    const add = document.createElement('button');
    add.type = 'button';
    add.textContent = '+ FIELD';
    add.addEventListener(BROWSER_EVENT.CLICK, () => {
      const field = missing.find(candidate => candidate.name === select.value);
      if(field) {
        this.commit([...path, field.name], initialFieldValue(field));
      }
    });

    form.append(select, add);
    return form;
  }

  /**
   * Create fields without source editing; arrays duplicate the last item as a useful starter.
   * @private @param path @param value
   */
  private addField(path: string[], value: JsonValue[] | Record<string, JsonValue>) {
    const form = document.createElement('div');
    form.className = 'property-add';

    const name = document.createElement('input');
    name.placeholder = 'Custom field';
    name.setAttribute('aria-label', `New property in ${path.join('.')}`);

    const type = document.createElement('select');
    type.setAttribute('aria-label', 'Property type');
    for(const text of ['number', 'text', 'boolean', 'group', 'array']) {
      type.append(option(text, text));
    }

    const add = document.createElement('button');
    add.type = 'button';
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
          this.actions.error('Use a unique property name.');
          return;
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
    button.type = 'button';
    button.className = 'remove-property';
    button.textContent = '−';
    button.title = `Remove ${path.join('.')}`;
    button.setAttribute('aria-label', button.title);
    button.addEventListener(BROWSER_EVENT.CLICK, () => this.commit(path, undefined));
    return button;
  }

  /**
   * List a component's issues at the top of its group so missing required fields are visible too.
   * @private
   */
  private appendIssues(parent: HTMLElement, component: string) {
    const issues = this.issues.filter(issue => issue.component === component);
    if(issues.length === 0) {
      return;
    }
    const list = document.createElement('ul');
    list.className = 'property-issues';
    list.setAttribute('role', 'status');
    for(const issue of issues) {
      const item = document.createElement('li');
      item.textContent = issue.message;
      list.append(item);
    }

    // Issues sit below the component description and above its fields.
    const description = parent.querySelector(':scope > .component-description');
    if(description) {
      description.after(list);
    } else {
      parent.prepend(list);
    }
  }

  /**
   * Flag a control that has an issue, or that its schema ignores under the current sibling values.
   * @private
   */
  private markState(element: HTMLElement, path: string[], schema: ComponentFieldSchema | null, container: JsonRecord | null) {
    element.dataset.fieldPath = path.join('.');
    if(schema?.when && container && !isComponentFieldActive(schema, container)) {
      element.classList.add('inactive');
      element.dataset.inactiveReason = `Unused while ${schema.when.field} is ${String(container[schema.when.field])}.`;
    }
    this.markIssue(element, path);
  }

  /**
   * Toggle the issue flag on one rendered field; the tooltip shows the issue, else any inactive reason.
   * @private
   */
  private markIssue(element: HTMLElement, path: string[]) {
    const fieldPath = path.slice(1).join('.');
    const issue = this.issues.find(candidate => candidate.component === path[0] && candidate.field === fieldPath);
    element.classList.toggle('has-issue', Boolean(issue));
    element.title = issue?.message ?? element.dataset.inactiveReason ?? '';
  }

  /**
   * Resolve the schema for a field path below a component, or null for components and custom data.
   * @private
   */
  private fieldSchema(path: string[]): ComponentFieldSchema | null {
    if(path.length < 2) {
      return null;
    }
    return this.actions.schemas.field(path[0], path.slice(1));
  }

  /**
   * Return the schema fields that describe a record: a component's fields or a list item's fields.
   * @private
   */
  private recordFields(path: string[]): readonly ComponentFieldSchema[] | null {
    if(path.length === 1) {
      return this.actions.schemas.get(path[0])?.fields ?? null;
    }
    const list = this.actions.schemas.field(path[0], path.slice(1, -1));
    const isListItem = list?.kind === COMPONENT_FIELD_KIND.LIST && /^\d+$/.test(path.at(-1) ?? '');
    return isListItem ? list?.items ?? null : null;
  }

  /** @private @param path @param value */
  private commit(path: string[], value: JsonValue | undefined) {
    try {
      this.capture();
      this.actions.change(path, value);
    } catch(error) {
      this.actions.error(error instanceof Error ? error.message : String(error));
    }
  }
}

/**
 * Decide whether a value uses the compact vector control: schema vectors and colors, or
 * custom triples whose names follow the engine's spatial and color conventions.
 */
function isVectorValue(schema: ComponentFieldSchema | null, path: string[], value: JsonValue): value is number[] {
  const isNumberList = Array.isArray(value) && [3, 4].includes(value.length) && value.every(item => typeof item === 'number');
  if(!isNumberList) {
    return false;
  }
  if(schema) {
    return schema.kind === COMPONENT_FIELD_KIND.VECTOR3 || schema.kind === COMPONENT_FIELD_KIND.COLOR;
  }
  return CUSTOM_VECTOR_NAMES.includes(path.at(-1) ?? '');
}

/**
 * Order record keys with schema fields first (schema order), then custom keys (authored order).
 * Array indices keep their order.
 */
function orderedKeys(value: JsonValue[] | JsonRecord, fields: readonly ComponentFieldSchema[] | null): string[] {
  const keys = Object.keys(value);
  if(!fields || Array.isArray(value)) {
    return keys;
  }
  const schemaKeys = fields.map(field => field.name).filter(name => Object.hasOwn(value, name));
  return [...schemaKeys, ...keys.filter(key => !schemaKeys.includes(key))];
}

/**
 * Label a group: schema labels for components and fields, `#n` for list items, and names otherwise.
 */
function groupLabel(path: string[], schemaLabel: string | undefined): string {
  const name = path.at(-1) ?? '';
  if(schemaLabel) {
    return schemaLabel;
  }
  if(path.length > 1 && /^\d+$/.test(name)) {
    return `#${Number(name) + 1}`;
  }
  return name.replaceAll('_', ' ');
}

/**
 * Starting value for a schema field the author adds: its runtime default when declared, otherwise
 * the smallest valid value of its kind.
 */
function initialFieldValue(field: ComponentFieldSchema): JsonValue {
  if(field.default !== undefined) {
    return structuredClone(field.default);
  }
  switch(field.kind) {
    case COMPONENT_FIELD_KIND.NUMBER: {
      const minimum = field.min ?? 0;
      return field.minExclusive ? minimum + (field.step ?? 1) : minimum;
    }
    case COMPONENT_FIELD_KIND.BOOLEAN:
      return false;
    case COMPONENT_FIELD_KIND.CHOICE:
      return field.choices?.[0] ?? '';
    case COMPONENT_FIELD_KIND.VECTOR3:
      return [0, 0, 0];
    case COMPONENT_FIELD_KIND.COLOR:
      return [1, 1, 1];
    case COMPONENT_FIELD_KIND.RECORD:
      return {};
    case COMPONENT_FIELD_KIND.LIST:
      return [];
  }
  return '';
}

/** @param value @param text */
function option(value: string, text: string) {
  const node = document.createElement('option');
  node.value = value;
  node.textContent = text;
  return node;
}
