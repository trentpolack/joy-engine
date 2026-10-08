// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { ENTITY_TYPE, TRANSFORM_PROPERTY } from './constants.ts';
import type { JsonValue, JsonRecord, EntityOptions, EntityDefinition, EntityVector3 } from './types.ts';
import { assetPath } from './asset-path.ts';

export type { JsonValue };
export type { JsonRecord };

/**
 * Validate and copy a definition, applying defaults without retaining authored references.
 * Component values must be finite JSON records; custom keys survive unchanged.
 * @param definition
 */
export function copyEntityDefinition(definition: EntityOptions): EntityDefinition {
  if(!definition || typeof definition !== 'object') {
    throw new TypeError('Entity definition must be an object.');
  }
  const {id, name = id, type = ENTITY_TYPE.ENTITY, tags = [], transform = {}, components = {}} = definition;
  if(typeof id !== 'string' || !id.trim()) {
    throw new TypeError('Entity id must be a nonempty string.');
  }
  if(typeof name !== 'string' || typeof type !== 'string' || !type.trim()) {
    throw new TypeError('Entity name and type must be strings; type cannot be empty.');
  }
  if(!Array.isArray(tags) || tags.some((tag) => typeof tag !== 'string')) {
    throw new TypeError('Entity tags must be strings.');
  }
  if(!isRecord(transform) || !isRecord(components)) {
    throw new TypeError('Entity transform and components must be records.');
  }

  const componentCopies: Record<string, JsonRecord> = {};
  for(const [key, value] of Object.entries(components)) {
    if(!isRecord(value)) {
      throw new TypeError(`Component ${key} must be a JSON record.`);
    }
    // Define own properties so author-defined keys such as __proto__ remain data.
    Object.defineProperty(componentCopies, key, {
      value: copyJsonRecord(value, new Set()),
      enumerable: true,
      writable: true,
      configurable: true
    });
  }
  const template = definition.template === undefined ? undefined : assetPath(definition.template, ['.joyobject']);
  if(definition.overrides !== undefined && (!template || !isRecord(definition.overrides))) {
    throw new TypeError('Object overrides require a template and a JSON record.');
  }
  const overrides = definition.overrides === undefined ? undefined : copyJsonRecord(definition.overrides, new Set());
  if(overrides && Object.keys(overrides).some(key => !['type', 'tags', 'components'].includes(key))) {
    throw new TypeError('Object overrides support type, tags and components only.');
  }
  return {
    ...(template ? {template, overrides: overrides ?? {}} : {}),
    id, name, type, tags: [...tags],
    transform: {
      position: copyVector(transform.position === undefined ? [0, 0, 0] : transform.position, TRANSFORM_PROPERTY.POSITION),
      rotation: copyVector(transform.rotation === undefined ? [0, 0, 0] : transform.rotation, TRANSFORM_PROPERTY.ROTATION),
      scale: copyVector(transform.scale === undefined ? [1, 1, 1] : transform.scale, TRANSFORM_PROPERTY.SCALE)
    },
    components: componentCopies
  };
}

/**
 * Accept plain component records, including records without a prototype.
 * @param value
 */
function isRecord(value: unknown): value is Record<string, unknown> {
  if(value === null || typeof value !== 'object' || Array.isArray(value)) {
    return false;
  }
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

/**
 * Copy exactly three finite components; sparse arrays are rejected.
 * @param value
 * @param label Transform field used in validation errors.
 */
function copyVector(value: unknown, label: string): EntityVector3 {
  if(!Array.isArray(value) || value.length !== 3 || !Array.from(value).every((entry) => typeof entry === 'number' && Number.isFinite(entry))) {
    throw new TypeError(`Entity ${label} must contain three finite numbers.`);
  }
  return [value[0], value[1], value[2]];
}

/**
 * Copy component fields while preserving authored keys and rejecting cycles.
 * @param value
 * @param ancestors Active recursion path, shared with nested copies.
 */
function copyJsonRecord(value: Record<string, unknown>, ancestors: Set<object>): JsonRecord {
  if(ancestors.has(value)) {
    throw new TypeError('Component JSON must not be cyclic.');
  }
  ancestors.add(value);

  const result: JsonRecord = {};
  for(const [key, entry] of Object.entries(value)) {
    Object.defineProperty(result, key, {
      value: copyJson(entry, ancestors),
      enumerable: true,
      writable: true,
      configurable: true
    });
  }
  ancestors.delete(value);
  return result;
}

/**
 * Copy finite JSON data; repeated references are allowed outside the active ancestry.
 * @param value
 * @param ancestors Active recursion path used to detect cycles.
 */
function copyJson(value: unknown, ancestors: Set<object>): JsonValue {
  if(value === null || typeof value === 'string' || typeof value === 'boolean') {
    return value;
  }
  if(typeof value === 'number' && Number.isFinite(value)) {
    return value;
  }
  if(Array.isArray(value)) {
    if(ancestors.has(value)) {
      throw new TypeError('Component JSON must not be cyclic.');
    }
    ancestors.add(value);
    const result = Array.from(value, (entry) => copyJson(entry, ancestors));
    ancestors.delete(value);
    return result;
  }
  if(isRecord(value)) {
    return copyJsonRecord(value, ancestors);
  }
  throw new TypeError('Component values must be finite JSON data.');
}
