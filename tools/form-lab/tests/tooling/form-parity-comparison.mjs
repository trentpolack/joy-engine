// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Compare complete FORM output with frozen reference results. Topology remains exact;
// floating-point geometry allows the same absolute and relative tolerance as the original corpus.

export const NUMERIC_TOLERANCE = 1e-8;
const EXACT_INTEGER_ARRAYS = new Set(['triangles', 'indices', 'points', 'triangleMaterials', 'pointMaterials', 'faces']);
const EXACT_INTEGER_FIELDS = new Set(['size', 'materialIndex', 'line', 'column']);
const MISMATCH_PREVIEW_CHARACTERS = 180;

export function compareData(expected, actual) {
  return compareValue(expected, actual, '$', false);
}

/**
 * Recursive comparison behind `compareData`.
 * @param {unknown} expected
 * @param {unknown} actual
 * @param {string} path JSONPath-like location used in mismatch messages.
 * @param {boolean} exactNumbers True inside exact integer fields.
 * @returns {string | null}
 */
function compareValue(expected, actual, path, exactNumbers) {
  if(typeof expected !== typeof actual || expected === null || actual === null) {
    return expected === actual ? null : mismatch(path, expected, actual);
  }
  if(typeof expected === 'number') {
    return compareNumber(expected, /** @type {number} */ (actual), path, exactNumbers);
  }
  if(Array.isArray(expected) || Array.isArray(actual)) {
    return compareArray(expected, actual, path, exactNumbers);
  }
  if(typeof expected === 'object') {
    return compareObject(/** @type {Record<string, unknown>} */ (expected), /** @type {Record<string, unknown>} */ (actual), path, exactNumbers);
  }
  return expected === actual ? null : mismatch(path, expected, actual);
}

/**
 * @param {number} expected
 * @param {number} actual
 * @param {string} path
 * @param {boolean} exactNumbers
 * @returns {string | null}
 */
function compareNumber(expected, actual, path, exactNumbers) {
  if(!Number.isFinite(expected) || !Number.isFinite(actual)) {
    return mismatch(path, expected, actual);
  }
  const tolerance = exactNumbers ? 0 : Math.max(NUMERIC_TOLERANCE, NUMERIC_TOLERANCE*Math.max(Math.abs(expected), Math.abs(actual)));
  if(Math.abs(expected - actual) <= tolerance) {
    return null;
  }
  return `${mismatch(path, expected, actual)}; tolerance=${tolerance}`;
}

/**
 * @param {unknown} expected
 * @param {unknown} actual
 * @param {string} path
 * @param {boolean} exactNumbers
 * @returns {string | null}
 */
function compareArray(expected, actual, path, exactNumbers) {
  if(!Array.isArray(expected) || !Array.isArray(actual)) {
    return `${path}: array/object type differs`;
  }
  if(expected.length !== actual.length) {
    return `${path}.length: expected ${expected.length}, actual ${actual.length}`;
  }
  for(let index = 0; index < expected.length; index++) {
    const difference = compareValue(expected[index], actual[index], `${path}[${index}]`, exactNumbers);
    if(difference) {
      return difference;
    }
  }
  return null;
}

/**
 * @param {Record<string, unknown>} expected
 * @param {Record<string, unknown>} actual
 * @param {string} path
 * @param {boolean} exactNumbers
 * @returns {string | null}
 */
function compareObject(expected, actual, path, exactNumbers) {
  const expectedKeys = Object.keys(expected).sort();
  const actualKeys = Object.keys(actual).sort();
  const sameKeys = expectedKeys.length === actualKeys.length && expectedKeys.every((key, index) => key === actualKeys[index]);
  if(!sameKeys) {
    return `${path}: keys differ; expected ${JSON.stringify(expectedKeys)}, actual ${JSON.stringify(actualKeys)}`;
  }
  for(const key of expectedKeys) {
    const exact = EXACT_INTEGER_FIELDS.has(key) || (EXACT_INTEGER_ARRAYS.has(key) && Array.isArray(expected[key]));
    const difference = compareValue(expected[key], actual[key], `${path}.${key}`, exact);
    if(difference) {
      return difference;
    }
  }
  return null;
}

/**
 * @param {string} path
 * @param {unknown} expected
 * @param {unknown} actual
 * @returns {string}
 */
function mismatch(path, expected, actual) {
  return `${path}: expected ${preview(expected)}, actual ${preview(actual)}`;
}

/** @param {unknown} value @returns {string} */
function preview(value) {
  const text = JSON.stringify(value) ?? String(value);
  return text.length > MISMATCH_PREVIEW_CHARACTERS ? `${text.slice(0, MISMATCH_PREVIEW_CHARACTERS - 3)}...` : text;
}
