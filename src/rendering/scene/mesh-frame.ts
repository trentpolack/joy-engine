// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { MeshAsset } from './mesh-asset.ts';
import type { MeshInstance } from './mesh-instance.ts';

import { meshTransform, transformPoint } from './mesh-transform.ts';
export interface PreparedMesh {
  asset: MeshAsset;
  signature: number[];
  transform: number[];
  vertices: number[];
}

/** Cache world-space geometry per instance. Static meshes avoid repeated vertex transforms;
 * public array edits are detected without requiring callers to manage dirty flags.
 * @param instance
 * @param cache Scene-owned cache.
 */
export function prepareMesh(instance: MeshInstance, cache: WeakMap<MeshInstance, PreparedMesh>) {
  const signature = [...instance.position, ...instance.rotation, ...instance.scale, ...instance.tint, instance.opacity];
  if(signature.length !== 13 || !signature.every(Number.isFinite)) {
    throw new RangeError('Mesh instance needs finite XYZ transforms, RGB tint and opacity.');
  }
  if(instance.opacity < 0 || instance.opacity > 1) {
    throw new RangeError('Instance opacity must be between zero and one.');
  }
  const previous = cache.get(instance);
  if(previous?.asset === instance.asset && signature.every((value, index) => value === previous.signature[index])) {
    return previous;
  }
  const transform = meshTransform(instance);
  const geometry = instance.asset.geometry;
  const vertices = [];
  for(const index of geometry.indices) {
    const offset = index*3;
    const position = transformPoint(transform, geometry.positions[offset], geometry.positions[offset + 1], geometry.positions[offset + 2]);
    vertices.push(position[0], position[1], position[2],
      (geometry.colors[offset] ?? 1)*instance.tint[0],
      (geometry.colors[offset + 1] ?? 1)*instance.tint[1],
      (geometry.colors[offset + 2] ?? 1)*instance.tint[2],
      (geometry.alphas[index] ?? 1)*instance.opacity);
  }
  const prepared = {asset: instance.asset, signature, transform, vertices};
  cache.set(instance, prepared);
  return prepared;
}
