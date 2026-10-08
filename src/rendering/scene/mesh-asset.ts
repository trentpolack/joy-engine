// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { MeshAssetData } from '../../core/types.ts';

/** Immutable CPU geometry shared by any number of scenes and instances.
 * Owns copies, never GPU resources. Drop references to release an asset.
 */
export class MeshAsset {
  declare geometry: Readonly<{ positions: readonly number[]; indices: readonly number[]; normals: readonly number[]; uvs: readonly number[]; colors: readonly number[]; alphas: readonly number[]; triangleMaterials: readonly number[]; materials: readonly unknown[]; metadata: Readonly<Record<string, unknown>>; }>;
  declare bounds: Readonly<{ min: readonly number[]; max: readonly number[]; }>;
  declare hasTransparency: boolean;

  /** @param data Local Y-up geometry. */
  constructor(data: MeshAssetData) {
    const positions = copyNumbers(data.positions, 'positions');
    const indices = copyNumbers(data.indices, 'indices');
    if(positions.length % 3 || indices.length % 3) {
      throw new RangeError('Mesh positions and triangle indices must be triples.');
    }
    const count = positions.length/3;
    if(indices.some(index => !Number.isInteger(index) || index < 0 || index >= count)) {
      throw new RangeError('Mesh index is outside the vertex array.');
    }
    const normals = attribute(data.normals, count*3, 'normals');
    const uvs = attribute(data.uvs, count*2, 'uvs');
    const colors = attribute(data.colors, count*3, 'colors');
    const alphas = attribute(data.alphas, count, 'alphas');
    if(alphas.some(alpha => alpha < 0 || alpha > 1)) {
      throw new RangeError('Mesh alpha must be between zero and one.');
    }
    const materials = freezeData(structuredClone(data.materials ?? []));
    const triangleMaterials = attribute(data.triangleMaterials, indices.length/3, 'triangleMaterials');
    if(triangleMaterials.some(index => !Number.isInteger(index) || index < 0 || index >= materials.length)) {
      throw new RangeError('Mesh material index is outside the material array.');
    }
    this.geometry = Object.freeze({positions, indices, normals, uvs, colors, alphas,
      triangleMaterials, materials, metadata: freezeData(structuredClone(data.metadata ?? {}))});
    const min = [0, 0, 0];
    const max = [0, 0, 0];
    if(count) {
      min.fill(Infinity);
      max.fill(-Infinity);
      for(let index = 0; index < positions.length; index++) {
        const axis = index % 3;
        min[axis] = Math.min(min[axis], positions[index]);
        max[axis] = Math.max(max[axis], positions[index]);
      }
    }
    this.bounds = Object.freeze({min: Object.freeze(min), max: Object.freeze(max)});
    this.hasTransparency = alphas.some(alpha => alpha < 1);
    Object.freeze(this);
  }
}

/** @param values @param name */
function copyNumbers(values: readonly number[], name: string) {
  const copy = Array.from(values);
  if(copy.some(value => !Number.isFinite(value))) {
    throw new RangeError(`Mesh ${name} must contain finite numbers.`);
  }
  return Object.freeze(copy);
}
/** @param values @param length @param name */
function attribute(values: readonly number[] | undefined, length: number, name: string) {
  const copy = copyNumbers(values ?? [], name);
  if(copy.length && copy.length !== length) {
    throw new RangeError(`Mesh ${name} must align with its geometry.`);
  }
  return copy;
}
/** Deep-freeze structured authoring records without retaining caller-owned references.
 * @param value @param [seen] */
function freezeData<T>(value: T, seen: WeakSet<object> = new WeakSet()): T {
  if(value && typeof value === 'object') {
    if(!Array.isArray(value) && Object.getPrototypeOf(value) !== Object.prototype && Object.getPrototypeOf(value) !== null) {
      throw new TypeError('Mesh authoring data must use plain records and arrays.');
    }
    if(seen.has(value)) {
      return value;
    }
    seen.add(value);
    for(const child of Object.values(value)) {
      freezeData(child, seen);
    }
    Object.freeze(value);
  }
  return value;
}
