// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { MeshAsset } from './mesh-asset.ts';
import type { MeshInstanceOptions } from '../../core/types.ts';
import type { RenderScene } from './render-scene.ts';
import type { MeshInstance } from './mesh-instance.ts';

/** Scene-owned stable-key collection. beginUpdate/set/endUpdate prunes absent views. */
export class MeshCollection {
  declare scene: RenderScene;
  private declare entries: Map<string | number | object, MeshInstance>;
  private declare seen: Set<string | number | object>;
  declare updating: boolean;

  /** @param scene Borrowed owner. */
  constructor(scene: RenderScene) {
    this.scene = scene;
    /** @private */
    this.entries = new Map();
    /** @private */
    this.seen = new Set();
    this.updating = false;
  }

  /** Start one synchronization pass; set() marks the instances that should survive. */
  beginUpdate() {
    this.scene.assertAlive();
    if(this.updating) {
      throw new Error('Mesh collection update is already open.');
    }
    this.seen.clear();
    this.updating = true;
  }

  /** Create or update a persistent instance. Use stable gameplay IDs or record references, not array positions.
   * @param key @param asset
   * @param [options]
   */
  set(key: string | number | object, asset: MeshAsset, options: MeshInstanceOptions = {}) {
    this.scene.assertAlive();
    if(!this.updating) {
      throw new Error('Call beginUpdate before submitting collection meshes.');
    }
    let instance = this.entries.get(key);
    if(instance && this.scene.instances.has(instance)) {
      instance.set(options);
      instance.asset = asset;
    } else {
      instance = this.scene.createMesh(asset, options);
      this.entries.set(key, instance);
    }
    this.seen.add(key);
    return instance;
  }

  /** Remove instances omitted from the open pass without releasing their borrowed assets. */
  endUpdate() {
    if(!this.updating) {
      throw new Error('Mesh collection update is not open.');
    }
    for(const [key, instance] of this.entries) {
      if(!this.seen.has(key)) {
        this.scene.remove(instance);
        this.entries.delete(key);
      }
    }
    this.seen.clear();
    this.updating = false;
  }

  /** Remove all collection instances and abandon any open synchronization pass. */
  clear() {
    for(const instance of this.entries.values()) {
      this.scene.remove(instance);
    }
    this.entries.clear();
    this.seen.clear();
    this.updating = false;
  }
}
