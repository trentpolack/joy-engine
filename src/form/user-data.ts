// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { FORM_VALUE_KIND } from './constants.ts';
import type { ScriptValue } from './procedural-runtime.ts';

export type DataValue = null | boolean | number | string | DataValue[] | {
    [key: string]: DataValue;
};
export interface UserData {
  [key: string]: DataValue;
}
/** Internal tag keeps authored records distinct from mesh/material/geometry handles. */
export interface DataExpression {
  kind: typeof FORM_VALUE_KIND.DATA;
  value: DataValue;
}

export const USER_DATA_LIMITS = Object.freeze({depth: 32, values: 250000, characters: 2000000});

/** One evaluation owns this budget, including literal, vertex, and output copies. */
export class UserDataStore {
  declare values: number;
  declare characters: number;

  constructor() {
    this.values = 0;
    this.characters = 0;
  }

  /** Validate and copy JSON-shaped data. No input object is retained or mutated.
   * @param value @param [depth] */
  copy(value: DataValue, depth: number = 0): DataValue {
    this.values++;
    if(depth > USER_DATA_LIMITS.depth || this.values > USER_DATA_LIMITS.values) {
      throw new Error('User data allocation limit exceeded (depth or values).');
    }
    if(typeof value === 'string') {
      this.reserveText(value);
      return value;
    }
    if(value === null || typeof value === 'boolean' || typeof value === 'number') {
      if(typeof value === 'number' && !Number.isFinite(value)) {
        throw new Error('User data numbers must be finite.');
      }
      return value;
    }
    if(Array.isArray(value)) {
      return value.map(item => this.copy(item, depth + 1));
    }

    const result: UserData = {};
    for(const [key, item] of Object.entries(value)) {
      validateDataKey(key);
      this.reserveText(key);
      result[key] = this.copy(item, depth + 1);
    }
    return result;
  }

  /** @private @param value */
  private reserveText(value: string) {
    this.characters+= value.length;
    if(this.characters > USER_DATA_LIMITS.characters) {
      throw new Error('User data allocation limit exceeded (text).');
    }
  }
}

/** Unwrap authored data without admitting runtime resource handles.
 * @param value */
export function dataValue(value: ScriptValue): DataValue {
  if(typeof value === 'number' || typeof value === 'string' || Array.isArray(value)) {
    return value;
  }
  if(value.kind === FORM_VALUE_KIND.DATA) {
    return value.value;
  }
  throw new Error('User data accepts records, arrays, text, finite numbers, booleans, and null; resource handles are not data.');
}

/** Require a record or null when attaching data to geometry.
 * @param value */
export function dataRecord(value: ScriptValue): UserData | null {
  const data = dataValue(value);
  if(data === null || (typeof data === 'object' && !Array.isArray(data))) {
    return data;
  }
  throw new Error('userData expects a record or null.');
}

/** @param name */
export function validateDataKey(name: string) {
  if(!name.length || name.length > 64 || ['__proto__', 'prototype', 'constructor'].includes(name)) {
    throw new Error('Invalid user data field name (1–64 characters; prototype keys are reserved).');
  }
}
