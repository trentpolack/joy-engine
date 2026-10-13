// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { AffineTransform, GeometryBounds } from '../../core/types.ts';
import type { SceneNode } from './scene-node.ts';
import type { MeshAsset } from './mesh-asset.ts';
import type { MeshInstance } from './mesh-instance.ts';
import type { RenderScene } from './render-scene.ts';

import { MESH_PASS } from './mesh-instance.ts';
import { transformBounds } from './mesh-transform.ts';

/** Representation-independent world snapshot consumed by color, depth, and shadow adapters. */
export interface SceneGeometrySnapshot {
  readonly identity: SceneNode;
  readonly node: SceneNode;
  readonly localBounds: GeometryBounds;
  readonly worldBounds: GeometryBounds;
  readonly worldTransform: AffineTransform;
  readonly worldRevision: number;
  readonly coverage: 'opaque' | 'transparent';
  readonly castsShadow: boolean;
  readonly receivesShadow: boolean;
}

/** Mesh adapter data borrows authored asset channels without copying them. */
export interface PreparedMeshGeometry extends SceneGeometrySnapshot {
  readonly instance: MeshInstance;
  readonly asset: MeshAsset;
}

/** Traverse world transforms once, then collect visible geometry independently of camera culling. */
export function prepareSceneGeometry(scene: RenderScene): PreparedMeshGeometry[] {
  scene.updateWorldTransforms();
  const snapshots: PreparedMeshGeometry[] = [];
  for(const instance of scene.instances) {
    const world = instance.preparedWorldState();
    if(!world.worldVisible || instance.opacity === 0) {
      continue;
    }
    const transparent = instance.pass === MESH_PASS.TRANSPARENT || (instance.pass === MESH_PASS.AUTO && (instance.opacity < 1 || instance.asset.hasTransparency));
    snapshots.push({identity: instance.identity, node: instance, instance, asset: instance.asset,
      localBounds: instance.asset.bounds, worldBounds: transformBounds(instance.asset.bounds, world.worldTransform),
      worldTransform: world.worldTransform, worldRevision: world.worldRevision,
      coverage: transparent ? MESH_PASS.TRANSPARENT : MESH_PASS.OPAQUE,
      castsShadow: instance.castsShadow && !transparent, receivesShadow: instance.receivesShadow});
  }
  return snapshots;
}
