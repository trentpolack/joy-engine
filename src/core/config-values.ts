// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

export type ConfigApplyMode = 'live' | 'next-spawn' | 'restart';

export interface ConfigField {
  key: string;
  label: string;
  group: string;
  description: string;
  defaultValue: number;
  min: number;
  max: number;
  step: number;
  units: string;
  applyMode: ConfigApplyMode;
}

/**
 * Validate field metadata and merge partial overrides into a new complete record.
 * @param fields Numeric configuration contract.
 * @param [overrides] Partial saved or runtime values.
 * @returns Independently owned complete values.
 */
export function createConfigValues(fields: readonly ConfigField[], overrides: Readonly<Record<string, number>> = {}): Record<string, number> {
  const fieldMap = validateFields(fields);
  for(const key of Object.keys(overrides)) {
    if(!fieldMap.has(key)) {
      throw new RangeError(`Unknown config key "${key}".`);
    }
  }

  const values: Record<string, number> = {};
  for(const field of fields) {
    const value = Object.hasOwn(overrides, field.key) ? overrides[field.key] : field.defaultValue;
    validateValue(field, value);

    values[field.key] = value;
  }

  return values;
}

/**
 * Validate a serialized version-1 override document and return complete values.
 * @param fields Numeric configuration contract.
 * @param document Untrusted parsed JSON value.
 * @returns Independently owned complete values.
 */
export function createConfigDocumentValues(fields: readonly ConfigField[], document: unknown): Record<string, number> {
  if(!isRecord(document)) {
    throw new TypeError('Config document must be an object.');
  }
  const allowedKeys = new Set(['$schema', 'version', 'values']);
  for(const key of Object.keys(document)) {
    if(!allowedKeys.has(key)) {
      throw new RangeError(`Unknown config document key "${key}".`);
    }
  }

  if(document.version !== 1) {
    throw new RangeError('Config document version must be 1.');
  }
  if(Object.hasOwn(document, '$schema') && typeof document.$schema !== 'string') {
    throw new TypeError('Config document $schema must be a string.');
  }
  if(!isRecord(document.values)) {
    throw new TypeError('Config document values must be an object.');
  }

  return(createConfigValues(fields,
    document.values as Record<string, number>));
}

/**
 * Produce the strict JSON Schema used by editors and persistence boundaries.
 * Values remain optional because override files only store authored differences.
 * @param fields Numeric configuration contract.
 * @returns Independently owned JSON Schema draft 2020-12 record.
 */
export function createConfigSchema(fields: readonly ConfigField[]): object {
  validateFields(fields);

  const properties: Record<string, object> = {};
  for(const field of fields) {
    properties[field.key] = {
      type: field.step === 1 ? 'integer' : 'number',
      minimum: field.min,
      maximum: field.max,
      description: field.description,
    };
  }

  return {
    $schema: 'https://json-schema.org/draft/2020-12/schema',
    type: 'object',
    additionalProperties: false,
    required: ['version', 'values'],
    properties: {
      $schema: { type: 'string' },
      version: { const: 1 },
      values: { type: 'object', additionalProperties: false, properties },
    },
  };
}

/**
 * Validate the array of config fields for correctness and uniqueness.
 * @param fields
 * */
function validateFields(fields: readonly ConfigField[]): Map<string, ConfigField> {
  if(!Array.isArray(fields)) {
    throw new TypeError('Config fields must be an array.');
  }

  const fieldMap = new Map<string, ConfigField>();
  for(const field of fields) {
    if(!field || typeof field.key !== 'string' || field.key.length === 0) {
      throw new TypeError('Every config field requires a non-empty key.');
    }
    if(fieldMap.has(field.key)) {
      throw new RangeError(`Duplicate config key "${field.key}".`);
    }
    for(const property of ['label', 'group', 'description', 'units']) {
      if(typeof field[property as 'label' | 'group' | 'description' | 'units'] !== 'string') {
        throw new TypeError(`Config field "${field.key}" requires a string ${property}.`);
      }
    }
    if(!Number.isFinite(field.defaultValue) || !Number.isFinite(field.min) || !Number.isFinite(field.max) || !Number.isFinite(field.step)) {
      throw new RangeError(`Config field "${field.key}" numeric metadata must be finite.`);
    }
    if((field.min > field.max) || field.step <= 0) {
      throw new RangeError(`Config field "${field.key}" requires min <= max and a positive step.`);
    }
    if(!['live', 'next-spawn', 'restart'].includes(field.applyMode)) {
      throw new RangeError(`Config field "${field.key}" has an invalid applyMode.`);
    }

    validateValue(field, field.defaultValue);
    fieldMap.set(field.key, field);
  }

  return fieldMap;
}

/**
 * Validate a numeric value against its config field constraints.
 * @param field
 * @param value
 */
function validateValue(field: ConfigField, value: number): void {
  if(!Number.isFinite(value)) {
    throw new RangeError(`Config value "${field.key}" must be finite.`);
  } else if(value < field.min) {
    throw new RangeError(`Config value "${field.key}" is below its minimum of ${field.min}.`);
  } else if(value > field.max) {
    throw new RangeError(`Config value "${field.key}" exceeds its maximum of ${field.max}.`);
  }

  if(field.step === 1 && !Number.isInteger(value)) {
    throw new RangeError(`Config value "${field.key}" must be an integer.`);
  }
}

/**
 * Narrow object-like input while excluding null and arrays; prototypes are not restricted.
 * @param value
 * */
function isRecord(value: unknown): value is Record<string, unknown> {
  return(typeof value === 'object' && value !== null && !Array.isArray(value));
}
