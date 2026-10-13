// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { MeshAsset } from './mesh-asset.ts';
import type { MeshInstanceOptions, SceneNodeOptions } from '../../core/types.ts';
import type { PreparedMesh } from './mesh-frame.ts';

import { SceneNode } from './scene-node.ts';
import { MeshInstance } from './mesh-instance.ts';
import { MeshCollection } from './mesh-collection.ts';

/** Owns a transform hierarchy and presentation instances; all geometry assets remain borrowed. */
export class RenderScene {
  private readonly ownedNodes = new Set<SceneNode>();
  private readonly rootNodes = new Set<SceneNode>();
  private readonly meshInstances = new Set<MeshInstance>();
  private readonly collections = new Set<MeshCollection>();
  /** @internal Scene-owned world vertex cache. */
  frameCache = new WeakMap<MeshInstance, PreparedMesh>();
  destroyed = false;

  get nodes(): ReadonlySet<SceneNode> { return this.ownedNodes; }
  get roots(): ReadonlySet<SceneNode> { return this.rootNodes; }
  get instances(): ReadonlySet<MeshInstance> { return this.meshInstances; }

  /** Create a transform-only node; the explicit parent overrides options.parent when provided. */
  createNode(options: SceneNodeOptions = {}, parent: SceneNode | null = options.parent ?? null) {
    this.assertAlive();
    const node = new SceneNode({...options, parent}, this);
    this.registerNode(node);
    return node;
  }

  /** Create scene-owned presentation state borrowing the supplied asset. */
  createMesh(asset: MeshAsset, options: MeshInstanceOptions = {}) {
    this.assertAlive();
    const instance = new MeshInstance(asset, options, this);
    this.registerNode(instance);
    return instance;
  }

  /** Remove an owned node and its full subtree; repeated or foreign removal is harmless. */
  remove(node: SceneNode) {
    if(!this.ownedNodes.has(node)) {
      return;
    }
    const pending = [node];
    while(pending.length) {
      const current = pending.pop()!;
      for(const child of current.children) {
        pending.push(child);
      }
      this.ownedNodes.delete(current);
      this.rootNodes.delete(current);
      if(current instanceof MeshInstance) {
        this.meshInstances.delete(current);
        this.frameCache.delete(current);
      }
      current.invalidate();
    }
  }

  /** Create a scene-owned keyed collection for synchronizing simulation views. */
  createCollection() {
    this.assertAlive();
    const collection = new MeshCollection(this);
    this.collections.add(collection);
    return collection;
  }

  /** Invalidate owned nodes and release references without releasing borrowed assets. */
  clear() {
    for(const collection of this.collections) {
      collection.clear();
    }
    for(const node of this.rootNodes) {
      this.remove(node);
    }
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

  /** @internal Register a fully constructed geometry node owned by this scene. */
  registerNode(node: SceneNode) {
    this.assertAlive();
    if(node.scene !== this || node.removed) {
      throw new Error('Cannot register a foreign or removed scene node.');
    }
    this.ownedNodes.add(node);
    if(node instanceof MeshInstance) {
      this.meshInstances.add(node);
    }
    this.updateRootNode(node);
  }

  /** @internal Maintain roots after a validated parent change. */
  updateRootNode(node: SceneNode) {
    if(!this.ownedNodes.has(node)) {
      return;
    }
    if(node.parent) {
      this.rootNodes.delete(node);
    } else {
      this.rootNodes.add(node);
    }
  }

  /** @internal Update ancestors before descendants in one iterative scene traversal. */
  updateWorldTransforms() {
    this.assertAlive();
    const pending = Array.from(this.rootNodes);
    while(pending.length) {
      const node = pending.pop()!;
      node.updateWorldTransform();
      for(const child of node.children) {
        pending.push(child);
      }
    }
  }

  /** @internal */
  assertAlive() {
    if(this.destroyed) {
      throw new Error('RenderScene is destroyed.');
    }
  }
}
