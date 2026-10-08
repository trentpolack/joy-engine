// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { MeshAsset } from './mesh-asset.ts';
import type { MeshInstanceOptions } from '../../core/types.ts';
import type { PreparedMesh } from './mesh-frame.ts';

import { MeshInstance } from './mesh-instance.ts';
import { MeshCollection } from './mesh-collection.ts';

/** Owns presentation instances; assets are borrowed and remain valid after removal. */
export class RenderScene {
  declare instances: Set<MeshInstance>;
  private declare collections: Set<MeshCollection>;
  declare frameCache: WeakMap<MeshInstance, PreparedMesh>;
  declare destroyed: boolean;

  constructor() {
    /** Read-only to consumers; mutate through scene methods. */
    this.instances = new Set();
    /** @private */
    this.collections = new Set();
    /** @internal */
    this.frameCache = new WeakMap();
    this.destroyed = false;
  }

  /** Create mutable presentation state owned by this scene and borrowing the supplied asset.
   * @param asset
   * @param [options]
   */
  createMesh(asset: MeshAsset, options?: MeshInstanceOptions) {
    this.assertAlive();
    const instance = new MeshInstance(asset, options);
    this.instances.add(instance);
    return instance;
  }

  /** Remove an owned instance; repeated removal is harmless. @param instance */
  remove(instance: MeshInstance) {
    this.instances.delete(instance);
  }

  /** Create a scene-owned keyed collection for synchronizing simulation views. */
  createCollection() {
    this.assertAlive();
    const collection = new MeshCollection(this);
    this.collections.add(collection);
    return collection;
  }

  /** Release instance references; collections remain reusable until scene destruction. */
  clear() {
    for(const collection of this.collections) {
      collection.clear();
    }
    this.instances.clear();
    this.frameCache = new WeakMap();
  }

  /** Idempotent disposal. The scene owns no GPU resources or simulation objects. */
  destroy() {
    if(this.destroyed) {
      return;
    }
    this.clear();
    this.collections.clear();
    this.destroyed = true;
  }

  /** @internal */
  assertAlive() {
    if(this.destroyed) {
      throw new Error('RenderScene is destroyed.');
    }
  }
}
