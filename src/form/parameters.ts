// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { FORM_PARAMETER_TYPE, FORM_VALUE_KIND } from './constants.ts';
import type { MeshValue, MaterialValue } from './resources.ts';
import { readMesh, readMaterialValue } from './resources.ts';
export type ParameterValue = number | number[] | MeshValue | MaterialValue;
export interface NumericParameter {
  name: string;
  type: typeof FORM_PARAMETER_TYPE.SCALAR | typeof FORM_PARAMETER_TYPE.VECTOR | typeof FORM_PARAMETER_TYPE.COLOR;
  value: number | number[];
  defaultValue: number | number[];
  min: number;
  max: number;
  step: number;
}
export interface ResourceParameter {
  name: string;
  type: typeof FORM_PARAMETER_TYPE.MESH | typeof FORM_PARAMETER_TYPE.MATERIAL;
  value: MeshValue | MaterialValue;
  defaultValue: MeshValue | MaterialValue;
}
export type Parameter = NumericParameter | ResourceParameter;

/** Validate a declaration and copy/clamp its override. Vectors use XYZ; colors use normalized RGBA.
 * Metadata bounds apply to each component. Returned arrays are owned by the compiled document.
 * @param name @param values
 * @param metadata @param supplied
 */
export function createParameter(name: string, values: ParameterValue[], metadata: Record<string, ParameterValue>, supplied: ParameterValue): Parameter {
  const defaultValue = values[0];
  if(typeof defaultValue === 'object' && !Array.isArray(defaultValue)) {
    if(values.length !== 1 || Object.keys(metadata).length) {
      throw new Error('Mesh and material parameters do not accept numeric bounds.');
    }
    const read = defaultValue.kind === FORM_VALUE_KIND.MESH ? readMesh : readMaterialValue;
    return {name, type: defaultValue.kind, defaultValue: read(defaultValue), value: read(supplied)};
  }
  const type = Array.isArray(defaultValue)
    ? defaultValue.length === 3
      ? 'vector'
      : 'color'
    : 'scalar';
  const legacy = values.length === 4;
  const min = scalar(
    legacy ? values[1] : (metadata.min ?? (type === 'color' ? 0 : NaN)),
  );
  const max = scalar(
    legacy ? values[2] : (metadata.max ?? (type === 'color' ? 1 : NaN)),
  );
  const step = scalar(
    legacy ? values[3] : (metadata.step ?? (type === 'color' ? 0.01 : NaN)),
  );
  const defaults = components(defaultValue);
  if(
    type === 'color' &&
    (min < 0 || max > 1 || defaults.some((value) => value < 0 || value > 1))
  ) {
    throw new Error('Color parameters require components and bounds in 0–1.');
  }
  if(
    min > max ||
    step <= 0 ||
    defaults.some((value) => value < min || value > max)
  ) {
    throw new Error(
      'Parameter requires min ≤ default ≤ max and a positive step.',
    );
  }
  const suppliedComponents = components(supplied);
  if(
    Array.isArray(defaultValue) !== Array.isArray(supplied) ||
    suppliedComponents.length !== defaults.length
  ) {
    throw new Error('Parameter override must match the declared components.');
  }
  const clamped = suppliedComponents.map((value) =>
    Math.max(min, Math.min(max, value)),
  );
  return {
    name,
    type,
    min,
    max,
    step,
    defaultValue: Array.isArray(defaultValue)
      ? [...defaultValue]
      : defaultValue,
    value: Array.isArray(defaultValue) ? clamped : clamped[0],
  };
}

/** Require a finite scalar at numeric expression boundaries. @param value */
export function scalar(value: unknown) {
  if(typeof value !== 'number' || !Number.isFinite(value)) {
    throw new Error(
      'Expected a finite scalar; parameter metadata requires min, max, and step.',
    );
  }
  return value;
}

/** Return finite components for document parameters. @param value */
function components(value: ParameterValue) {
  if(typeof value === 'object' && !Array.isArray(value)) {
    throw new Error('Expected numeric parameter components.');
  }
  const result = Array.isArray(value) ? value : [value];
  if(Array.isArray(value) && value.length !== 3 && value.length !== 4) {
    throw new Error(
      'Parameters require three vector or four color components.',
    );
  }
  return Array.from(result, scalar);
}

/** Validate and own a parameter override at a document/runtime boundary. @param value */
export function readParameterValue(value: unknown): ParameterValue {
  if(typeof value === 'number' && Number.isFinite(value)) {
    return value;
  }
  if(Array.isArray(value) && (value.length === 3 || value.length === 4) && Array.from(value).every(v => typeof v === 'number' && Number.isFinite(v))) {
    return [...value];
  }
  if(value && typeof value === 'object' && 'kind' in value) {
    if(value.kind === FORM_VALUE_KIND.MESH) {
      return readMesh(value);
    }
    if(value.kind === FORM_VALUE_KIND.MATERIAL) {
      return readMaterialValue(value);
    }
  }
  throw new Error('Parameters require finite scalars, XYZ vectors, RGBA colors, meshes, or materials.');
}
