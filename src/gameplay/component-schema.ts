// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { COMPONENT_FIELD_KIND } from './constants.ts';
import type { EntityDefinition, JsonRecord, JsonValue } from './types.ts';
import { BUILT_IN_COMPONENT_SCHEMAS } from './built-in-component-schemas.ts';

export type ComponentFieldKind = typeof COMPONENT_FIELD_KIND[keyof typeof COMPONENT_FIELD_KIND];

/**
 * Show a field only while a sibling choice holds one of the listed values,
 * e.g. box half extents only while `shape` is `box`.
 */
export interface ComponentFieldCondition {
  readonly field: string;
  readonly equals: readonly string[];
}

/**
 * One authored field inside a component record. Numbers, vectors and colors share bounds per channel.
 * `default` is the value the runtime assumes when the field is omitted; it is never written for the author.
 */
export interface ComponentFieldSchema {
  /** Own property name inside the component record. */
  readonly name: string;
  readonly kind: ComponentFieldKind;
  /** Inspector label; the field name is used when omitted. */
  readonly label?: string;
  /** One sentence of authoring help. */
  readonly description?: string;
  /** Display unit for numbers and vectors, such as `m`, `kg` or `s`. */
  readonly unit?: string;
  /** Runtime value for an omitted field. Absent means omission carries no implied value. */
  readonly default?: JsonValue;
  /** Report omission (or an empty asset path) as an issue. */
  readonly required?: boolean;
  /** Inclusive lower bound, or exclusive when `minExclusive` is true. */
  readonly min?: number;
  readonly minExclusive?: boolean;
  /** Inclusive upper bound. */
  readonly max?: number;
  /** Suggested control increment; validation does not quantize to it. */
  readonly step?: number;
  readonly integer?: boolean;
  /** Accepted values for `choice` fields. */
  readonly choices?: readonly string[];
  /** Workspace asset kinds (file extensions without the dot) accepted by `asset` fields. */
  readonly assetKinds?: readonly string[];
  /** Fields of each record item in a `list` field. */
  readonly items?: readonly ComponentFieldSchema[];
  readonly when?: ComponentFieldCondition;
}

/**
 * A named starting record offered when an author adds a component, e.g. a box collider or a FORM mesh.
 * Preset ids are unique across a catalog.
 */
export interface ComponentPreset {
  readonly id: string;
  readonly label: string;
  readonly data: JsonRecord;
}

/**
 * Authoring contract for one component type. Schemas describe authored JSON for tools and validation;
 * runtime behavior stays with the systems that interpret the component. Fields outside the schema
 * remain valid custom data and round-trip unchanged.
 */
export interface ComponentSchema {
  readonly type: string;
  readonly label: string;
  readonly description: string;
  /** Display group, normally a `COMPONENT_CATEGORY` value. */
  readonly category: string;
  readonly fields: readonly ComponentFieldSchema[];
  /** Starting records offered when adding the component; at least one is required. */
  readonly presets: readonly ComponentPreset[];
  /** Component types this component needs on the same entity. */
  readonly requires?: readonly string[];
}

/**
 * One advisory validation result. `field` is a dotted path inside the component record
 * (`wheels.2.radius`), or null for whole-component problems.
 */
export interface ComponentIssue {
  readonly component: string;
  readonly field: string | null;
  readonly message: string;
}

export interface ComponentSchemaCatalogOptions {
  /** Register the engine's built-in schemas first; defaults to true. */
  builtIns?: boolean;
}

/**
 * Instance-owned set of component schemas for authoring tools and validation.
 * Registered schemas are copied and frozen; lookups return borrowed read-only records.
 * The catalog is data only: it never constructs runtime components or evaluates authored values.
 */
export class ComponentSchemaCatalog {
  // Registration order is display order.
  private readonly schemas: Map<string, ComponentSchema>;

  // Preset ids are unique across schemas so tools can address one starting record directly.
  private readonly presetOwners: Map<string, ComponentSchema>;

  /**
   * @param options Catalog setup; built-in engine schemas are registered unless disabled.
   */
  constructor({builtIns = true}: ComponentSchemaCatalogOptions = {}) {
    this.schemas = new Map();
    this.presetOwners = new Map();

    if(builtIns) {
      for(const schema of BUILT_IN_COMPONENT_SCHEMAS) {
        this.register(schema);
      }
    }
  }

  /**
   * Register a project schema. Invalid schemas, duplicate types and duplicate preset ids throw
   * before anything is registered.
   * @param schema Borrowed definition; the catalog keeps a frozen copy.
   * @returns This catalog for chained registration.
   */
  register(schema: ComponentSchema): this {
    const copy = deepFreeze(structuredClone(schema));
    validateSchema(copy);

    if(this.schemas.has(copy.type)) {
      throw new Error(`Component schema ${copy.type} is already registered.`);
    }
    for(const preset of copy.presets) {
      if(this.presetOwners.has(preset.id)) {
        throw new Error(`Component preset ${preset.id} is already registered.`);
      }
    }

    this.schemas.set(copy.type, copy);
    for(const preset of copy.presets) {
      this.presetOwners.set(preset.id, copy);
    }
    return this;
  }

  /** Return whether a schema describes this component type. */
  has(type: string): boolean {
    return this.schemas.has(type);
  }

  /**
   * Borrow the read-only schema for a component type.
   * @returns The frozen schema, or null for custom component types.
   */
  get(type: string): ComponentSchema | null {
    return this.schemas.get(type) ?? null;
  }

  /** Borrow every schema in registration order. */
  list(): readonly ComponentSchema[] {
    return [...this.schemas.values()];
  }

  /**
   * Create an owned component record from a preset.
   * @param presetId A preset id from any registered schema.
   * @returns The component type and a fresh, caller-owned copy of the preset data.
   */
  createComponent(presetId: string): { type: string; data: JsonRecord; } {
    const schema = this.presetOwners.get(presetId);
    const preset = schema?.presets.find(candidate => candidate.id === presetId);
    if(!schema || !preset) {
      throw new Error(`Unknown component preset: ${presetId}.`);
    }
    return {type: schema.type, data: structuredClone(preset.data) as JsonRecord};
  }

  /**
   * Resolve the field schema for a path inside a component record, descending through list items.
   * @param type Component type.
   * @param path Field path inside the component record, e.g. `['wheels', '0', 'radius']`.
   * @returns The borrowed field schema, or null for custom data and for paths ending on a list item.
   */
  field(type: string, path: readonly string[]): ComponentFieldSchema | null {
    let fields = this.schemas.get(type)?.fields;
    let field: ComponentFieldSchema | null = null;

    for(let index = 0; index < path.length; index++) {
      if(!fields) {
        return null;
      }
      field = fields.find(candidate => candidate.name === path[index]) ?? null;
      if(!field) {
        return null;
      }

      // A list item index selects one record; the segment after it names a field of that record.
      // A path that ends on the index addresses the record itself, which has no field schema.
      if(field.kind === COMPONENT_FIELD_KIND.LIST && index + 1 < path.length) {
        index++;
        if(index + 1 >= path.length) {
          return null;
        }
        fields = field.items;
        continue;
      }
      fields = undefined;
    }
    return field;
  }

  /**
   * Check one authored component against its schema. Custom component types have no issues.
   * @param type Component type.
   * @param data Borrowed authored record; never mutated.
   */
  validate(type: string, data: JsonValue): ComponentIssue[] {
    const schema = this.schemas.get(type);
    if(!schema) {
      return [];
    }
    if(!isRecord(data)) {
      return [{component: type, field: null, message: `${schema.label} must be a record.`}];
    }
    return validateFields(type, schema.fields, data, '');
  }

  /**
   * Check every schema-described component on an entity, plus required companion components.
   * @param entity Borrowed resolved definition; linked objects should be resolved first.
   */
  validateEntity(entity: EntityDefinition): ComponentIssue[] {
    const issues: ComponentIssue[] = [];
    for(const [type, data] of Object.entries(entity.components)) {
      issues.push(...this.validate(type, data));

      const schema = this.schemas.get(type);
      for(const required of schema?.requires ?? []) {
        if(!Object.hasOwn(entity.components, required)) {
          const requiredLabel = this.schemas.get(required)?.label ?? required;
          issues.push({component: type, field: null, message: `${schema?.label} requires a ${requiredLabel} component.`});
        }
      }
    }
    return issues;
  }
}

/**
 * Return whether a field applies to its record under the field's sibling condition.
 * @param field Field schema to test.
 * @param record Borrowed record that holds the field and its siblings.
 */
export function isComponentFieldActive(field: ComponentFieldSchema, record: JsonRecord): boolean {
  if(!field.when) {
    return true;
  }
  const value = record[field.when.field];
  return typeof value === 'string' && field.when.equals.includes(value);
}

/**
 * Validate the fields of one record, recursing into list items with an item schema.
 * @param component Component type for issue reporting.
 * @param fields Schema fields for this record.
 * @param record Borrowed authored record.
 * @param prefix Dotted path of this record inside the component, or empty for the component itself.
 */
function validateFields(component: string, fields: readonly ComponentFieldSchema[], record: JsonRecord, prefix: string): ComponentIssue[] {
  const issues: ComponentIssue[] = [];
  for(const field of fields) {
    if(!isComponentFieldActive(field, record)) {
      continue;
    }
    const path = `${prefix}${field.name}`;
    const label = sentenceStart(field.label ?? field.name);

    if(!Object.hasOwn(record, field.name)) {
      if(field.required) {
        issues.push({component, field: path, message: `${label} is required.`});
      }
      continue;
    }

    const value = record[field.name];
    const message = describeInvalidValue(field, value);
    if(message) {
      issues.push({component, field: path, message: `${label} ${message}`});
      continue;
    }

    // Only list items with a declared item schema have nested fields to check.
    if(field.kind === COMPONENT_FIELD_KIND.LIST && field.items && Array.isArray(value)) {
      for(let index = 0; index < value.length; index++) {
        const item = value[index];
        if(!isRecord(item)) {
          issues.push({component, field: `${path}.${index}`, message: `${label} item ${index + 1} must be a record.`});
          continue;
        }
        issues.push(...validateFields(component, field.items, item, `${path}.${index}.`));
      }
    }
  }
  return issues;
}

/**
 * Describe why a present value does not satisfy its field, as a predicate completing "<label> ...".
 * @param field Field schema.
 * @param value Borrowed authored value.
 * @returns A message fragment, or null when the value is acceptable.
 */
function describeInvalidValue(field: ComponentFieldSchema, value: JsonValue): string | null {
  switch(field.kind) {
    case COMPONENT_FIELD_KIND.NUMBER: {
      if(typeof value !== 'number' || !Number.isFinite(value)) {
        return 'must be a finite number.';
      }
      if(field.integer && !Number.isInteger(value)) {
        return 'must be a whole number.';
      }
      return describeOutOfRange(field, [value]);
    }

    case COMPONENT_FIELD_KIND.BOOLEAN:
      return typeof value === 'boolean' ? null : 'must be true or false.';

    case COMPONENT_FIELD_KIND.TEXT:
      return typeof value === 'string' ? null : 'must be text.';

    case COMPONENT_FIELD_KIND.CHOICE:
      return typeof value === 'string' && (field.choices ?? []).includes(value)
        ? null
        : `must be one of ${(field.choices ?? []).join(', ')}.`;

    case COMPONENT_FIELD_KIND.VECTOR3:
    case COMPONENT_FIELD_KIND.COLOR: {
      if(!Array.isArray(value) || value.length !== 3 || !value.every(entry => typeof entry === 'number' && Number.isFinite(entry))) {
        return 'must contain three finite numbers.';
      }
      return describeOutOfRange(field, value as number[]);
    }

    case COMPONENT_FIELD_KIND.ASSET:
      return describeInvalidAsset(field, value);

    case COMPONENT_FIELD_KIND.RECORD:
      return isRecord(value) ? null : 'must be a record.';

    case COMPONENT_FIELD_KIND.LIST:
      return Array.isArray(value) ? null : 'must be a list.';
  }
  return null;
}

/**
 * Check every numeric channel against the field's bounds.
 * @param field Field schema with optional bounds.
 * @param values Finite channel values.
 */
function describeOutOfRange(field: ComponentFieldSchema, values: readonly number[]): string | null {
  const channels = values.length > 1 ? 'values ' : '';
  const minimum = field.min;
  const maximum = field.max;

  if(minimum !== undefined) {
    const belowMinimum = values.some(value => field.minExclusive ? value <= minimum : value < minimum);
    if(belowMinimum) {
      return field.minExclusive
        ? `${channels}must be greater than ${minimum}.`
        : `${channels}must be at least ${minimum}.`;
    }
  }
  if(maximum !== undefined && values.some(value => value > maximum)) {
    return `${channels}must be at most ${maximum}.`;
  }
  return null;
}

/**
 * Accept an unassigned (empty) path for optional fields and require a matching extension otherwise.
 * @param field Asset field schema.
 * @param value Borrowed authored value.
 */
function describeInvalidAsset(field: ComponentFieldSchema, value: JsonValue): string | null {
  if(typeof value !== 'string') {
    return 'must be an asset path.';
  }
  if(!value) {
    return field.required ? 'needs an asset.' : null;
  }
  const kinds = field.assetKinds ?? [];
  if(kinds.length > 0 && !kinds.some(kind => value.endsWith(`.${kind}`))) {
    return `must reference a ${kinds.map(kind => `.${kind}`).join(' or ')} asset.`;
  }
  return null;
}

/**
 * Reject schemas that tools could not render or validate consistently.
 * @param schema Frozen schema copy.
 */
function validateSchema(schema: ComponentSchema): void {
  if(!schema || typeof schema.type !== 'string' || !schema.type.trim()) {
    throw new TypeError('Component schemas require a nonempty type.');
  }
  if(typeof schema.label !== 'string' || typeof schema.description !== 'string' || typeof schema.category !== 'string') {
    throw new TypeError(`Component schema ${schema.type} requires a label, description and category.`);
  }
  if(!Array.isArray(schema.presets) || schema.presets.length === 0) {
    throw new TypeError(`Component schema ${schema.type} requires at least one preset.`);
  }
  validateFieldList(schema.type, schema.fields);

  const presetIds = new Set<string>();
  for(const preset of schema.presets) {
    if(typeof preset.id !== 'string' || !preset.id.trim() || typeof preset.label !== 'string' || !isRecord(preset.data)) {
      throw new TypeError(`Component schema ${schema.type} presets require an id, label and record data.`);
    }
    if(presetIds.has(preset.id)) {
      throw new Error(`Component schema ${schema.type} repeats preset ${preset.id}.`);
    }
    presetIds.add(preset.id);
  }
}

/**
 * Validate field names and kind-specific metadata, recursing into list item schemas.
 * @param type Owning component type for error messages.
 * @param fields Field list to check.
 */
function validateFieldList(type: string, fields: readonly ComponentFieldSchema[]): void {
  if(!Array.isArray(fields)) {
    throw new TypeError(`Component schema ${type} requires a field list.`);
  }

  const kinds = Object.values(COMPONENT_FIELD_KIND) as string[];
  const names = new Set<string>();
  for(const field of fields) {
    if(typeof field.name !== 'string' || !field.name.trim() || names.has(field.name)) {
      throw new TypeError(`Component schema ${type} requires unique, nonempty field names.`);
    }
    names.add(field.name);

    if(!kinds.includes(field.kind)) {
      throw new TypeError(`Component field ${type}.${field.name} has unknown kind ${String(field.kind)}.`);
    }
    if(field.kind === COMPONENT_FIELD_KIND.CHOICE && !(field.choices?.length)) {
      throw new TypeError(`Choice field ${type}.${field.name} requires choices.`);
    }
    if(field.kind === COMPONENT_FIELD_KIND.ASSET && !(field.assetKinds?.length)) {
      throw new TypeError(`Asset field ${type}.${field.name} requires asset kinds.`);
    }
    if(field.items) {
      if(field.kind !== COMPONENT_FIELD_KIND.LIST) {
        throw new TypeError(`Only list fields may declare items (${type}.${field.name}).`);
      }
      validateFieldList(type, field.items);
    }
  }

  // Conditions refer to sibling choices so tools can toggle them without evaluating code.
  for(const field of fields) {
    if(!field.when) {
      continue;
    }
    const sibling = fields.find(candidate => candidate.name === field.when?.field);
    if(sibling?.kind !== COMPONENT_FIELD_KIND.CHOICE) {
      throw new TypeError(`Component field ${type}.${field.name} must depend on a sibling choice field.`);
    }
  }
}

/**
 * Capitalize a field label so it can open an issue sentence.
 * @param label Field label or name.
 */
function sentenceStart(label: string): string {
  return `${label.charAt(0).toUpperCase()}${label.slice(1)}`;
}

/**
 * Accept plain JSON records, including records without a prototype.
 * @param value Candidate value.
 */
function isRecord(value: unknown): value is JsonRecord {
  if(value === null || typeof value !== 'object' || Array.isArray(value)) {
    return false;
  }
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

/**
 * Freeze a copied schema graph so borrowed lookups cannot change the catalog.
 * @param value Owned copy.
 * @returns The same value, frozen.
 */
function deepFreeze<T>(value: T): T {
  if(value && typeof value === 'object') {
    for(const entry of Object.values(value)) {
      deepFreeze(entry);
    }
    Object.freeze(value);
  }
  return value;
}
