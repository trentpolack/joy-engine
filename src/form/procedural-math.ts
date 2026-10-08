// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { FORM_COMMAND } from './constants.ts';
import type { ScriptValue } from './procedural-runtime.ts';
import { scalar } from './parameters.ts';
export type { ScriptValue };

/** Component arithmetic broadcasts scalars; vector/color sizes must agree.
 * @param operator @param left @param right
 */
export function vectorArithmetic(operator: string, left: ScriptValue, right: ScriptValue) {
  if(!['+', '-', '*', '/'].includes(operator)) {
    throw new Error('Vector arithmetic supports +, -, *, and /; compare scalar components.');
  }
  const size = Array.isArray(left) ? left.length : Array.isArray(right) ? right.length : 1;
  if(Array.isArray(left) && Array.isArray(right) && left.length !== right.length) {
    throw new Error('Vector/color arithmetic requires matching component counts.');
  }
  return Array.from({length: size}, (_, index) => {
    const a = scalar(Array.isArray(left) ? left[index] : left);
    const b = scalar(Array.isArray(right) ? right[index] : right);
    if(operator === '+') {
      return a + b;
    }
    if(operator === '-') {
      return a - b;
    }
    return operator === '*' ? a*b : a/b;
  });
}
/** @param name @param values */
export function vectorFunction(name: string, values: ScriptValue[]): number | number[] | undefined {
  if(name === FORM_COMMAND.MIX && (Array.isArray(values[0]) || Array.isArray(values[1]))) {
    if(values.length !== 3) {
      throw new Error('mix expects two values and a scalar weight.');
    }
    const delta = vectorArithmetic('-', values[1], values[0]);
    return vectorArithmetic('+', values[0], vectorArithmetic('*', delta, scalar(values[2])));
  }
  if(!([FORM_COMMAND.LENGTH, FORM_COMMAND.NORMALIZE, FORM_COMMAND.DOT, FORM_COMMAND.CROSS] as readonly string[]).includes(name)) {
    return undefined;
  }
  const expected = name === FORM_COMMAND.DOT || name === FORM_COMMAND.CROSS ? 2 : 1;
  if(values.length !== expected || values.some(value => !Array.isArray(value) || value.length !== 3)) {
    throw new Error(`${name} expects ${expected} XYZ vector${expected > 1 ? 's' : ''}.`);
  }
  const [a, b] = ((values) as number[][]);
  if(name === FORM_COMMAND.LENGTH) {
    return Math.hypot(...a);
  }
  if(name === FORM_COMMAND.NORMALIZE) {
    const scale = Math.max(...a.map(Math.abs));
    if(scale === 0) {
      throw new Error('normalize expects a nonzero vector.');
    }
    const scaled = a.map(value => value/scale), length = Math.hypot(...scaled);
    return scaled.map(value => value/length);
  }
  if(name === FORM_COMMAND.DOT) {
    return a.reduce((sum, value, axis) => sum + value*b[axis], 0);
  }
  return [a[1]*b[2] - a[2]*b[1], a[2]*b[0] - a[0]*b[2], a[0]*b[1] - a[1]*b[0]];
}
/** Smooth deterministic value noise, [-1,1]. Third coordinate can be a seed slice.
 * @param x @param y @param z
 */
export function noise(x: number, y: number, z: number) {
  const base = [x, y, z].map(Math.floor);
  const blend = [x, y, z].map((value, axis) => {
    const t = value - base[axis];
    return t*t*(3 - 2*t);
  });
  let result = 0;
  for(let corner = 0; corner < 8; corner++) {
    const dx = corner & 1, dy = (corner >> 1) & 1, dz = (corner >> 2) & 1;
    let hash = Math.imul(base[0] + dx, 374761393) ^ Math.imul(base[1] + dy, 668265263) ^ Math.imul(base[2] + dz, 1442695041);
    hash = Math.imul(hash ^ (hash >>> 13), 1274126177);
    const sample = ((hash ^ (hash >>> 16)) >>> 0)/4294967295*2 - 1;
    result+= sample*(dx ? blend[0] : 1 - blend[0])*(dy ? blend[1] : 1 - blend[1])*(dz ? blend[2] : 1 - blend[2]);
  }
  return Math.max(-1, Math.min(1, result));
}
/** @param low @param high @param value */
export function smoothstep(low: number, high: number, value: number) {
  if(high <= low) {
    throw new Error('smoothstep requires low < high.');
  }
  const t = Math.max(0, Math.min(1, (value - low)/(high - low)));
  return t*t*(3 - 2*t);
}
