// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { AffineTransform3D, Color, Vector3 } from '../../core/types.ts';
import type { MeshAsset } from './mesh-asset.ts';
import type { MeshInstance } from './mesh-instance.ts';

import {
  meshTransform,
  transformPoint
} from './mesh-transform.ts';

/**
 * Snapshot used to invalidate cached geometry after public instance arrays change.
 * Positions use world units; XYZ Euler rotations use radians; tint is linear RGB.
 * Each vector is copied so in-place instance edits cannot change the cached snapshot.
 * Color groups RGB tint with instance opacity.
 */
interface MeshSignature {
  readonly position: Readonly<Vector3>;
  readonly rotation: Readonly<Vector3>;
  readonly scale: Readonly<Vector3>;
  readonly tintAndOpacity: Color;
}

/** Cached world-space geometry for one instance, owned by the scene's frame cache. */
export interface PreparedMesh {
  /** Borrowed source asset; replacing the asset also invalidates the cache. */
  readonly asset: MeshAsset;

  /** Owned value snapshot, compared on the next preparation to detect instance edits. */
  readonly signature: MeshSignature;

  /** Owned local-to-world transform, reused for bounds culling. */
  readonly transform: AffineTransform3D;

  /**
   * Owned non-indexed triangle vertices, packed as world XYZ and linear RGBA.
   * Seven floats per vertex; frames copy this buffer without modifying it.
   * Readonly protects the property reference, not the Float32Array's elements.
   */
  readonly vertices: Float32Array;
}

/**
 * Cache world-space geometry per instance. Static meshes avoid repeated vertex transforms;
 * public array edits are detected without requiring callers to manage dirty flags.
 * @param instance
 * @param cache Scene-owned cache.
 */
export function prepareMesh(instance: MeshInstance, cache: WeakMap<MeshInstance, PreparedMesh>): PreparedMesh {
  const signature = {
    position: [...instance.position],
    rotation: [...instance.rotation],
    scale: [...instance.scale],
    tintAndOpacity: [...instance.tint, instance.opacity]
  };
  if(!isValidMeshSignature(signature)) {
    throw new RangeError('Mesh instance needs finite XYZ transforms, RGB tint and opacity.');
  }
  if(instance.opacity < 0 || instance.opacity > 1) {
    throw new RangeError('Instance opacity must be between zero and one.');
  }
  const previous = cache.get(instance);
  if(previous?.asset === instance.asset && meshSignaturesMatch(signature, previous.signature)) {
    return previous;
  }
  const transform = meshTransform(instance);
  const geometry = instance.asset.geometry;
  // Packed once per transform change; frames copy it with TypedArray.set rather than per-value pushes.
  const vertices = new Float32Array(geometry.indices.length*7);
  let cursor = 0;
  for(const index of geometry.indices) {
    const offset = index*3;
    const position = transformPoint(transform, geometry.positions[offset], geometry.positions[offset + 1], geometry.positions[offset + 2]);
    vertices[cursor] = position[0];
    vertices[cursor + 1] = position[1];
    vertices[cursor + 2] = position[2];
    vertices[cursor + 3] = (geometry.colors[offset] ?? 1)*instance.tint[0];
    vertices[cursor + 4] = (geometry.colors[offset + 1] ?? 1)*instance.tint[1];
    vertices[cursor + 5] = (geometry.colors[offset + 2] ?? 1)*instance.tint[2];
    vertices[cursor + 6] = (geometry.alphas[index] ?? 1)*instance.opacity;
    cursor+= 7;
  }
  const prepared = {asset: instance.asset, signature, transform, vertices};
  cache.set(instance, prepared);
  return prepared;
}

/**
 * Validate copied components because callers can mutate the public instance arrays.
 */
function isValidMeshSignature(signature: {
  position: readonly number[];
  rotation: readonly number[];
  scale: readonly number[];
  tintAndOpacity: readonly number[];
}): signature is MeshSignature {
  return signature.position.length === 3 && signature.position.every(Number.isFinite)
    && signature.rotation.length === 3 && signature.rotation.every(Number.isFinite)
    && signature.scale.length === 3 && signature.scale.every(Number.isFinite)
    && signature.tintAndOpacity.length === 4 && signature.tintAndOpacity.every(Number.isFinite);
}

/**
 * Compare snapshot values rather than the identities of the copied vectors.
 */
function meshSignaturesMatch(current: MeshSignature, previous: MeshSignature): boolean {
  return current.position.every((value, index) => value === previous.position[index])
    && current.rotation.every((value, index) => value === previous.rotation[index])
    && current.scale.every((value, index) => value === previous.scale[index])
    && current.tintAndOpacity.every((value, index) => value === previous.tintAndOpacity[index]);
}
