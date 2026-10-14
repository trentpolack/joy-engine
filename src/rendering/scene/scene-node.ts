// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { AffineTransform, SceneNodeOptions } from '../../core/types.ts';
import type { RenderScene } from './render-scene.ts';

import { composeTransforms, meshTransform } from './mesh-transform.ts';

const TRANSFORM_FIELDS = ['position', 'rotation', 'scale'] as const;

/** Scene-owned transform node. Mutable local arrays support direct edits; world data is read-only. */
export class SceneNode {
  readonly scene: RenderScene | null;
  position = [0, 0, 0];
  rotation = [0, 0, 0];
  scale = [1, 1, 1];
  visible = true;

  private parentNode: SceneNode | null = null;
  private readonly childNodes = new Set<SceneNode>();
  private invalidated = false;
  private localSignature: number[] = [];
  private ownRevision = 0;
  private composedRevision = 0;
  private preparedOwnRevision = -1;
  private parentRevision = -1;
  private preparedParent: SceneNode | null = null;
  private affine: AffineTransform = [1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0];
  private inheritedVisible = true;

  constructor(options: SceneNodeOptions = {}, scene: RenderScene | null = null) {
    this.scene = scene;
    this.setNodeOptions(options);
  }

  /** Stable identity shared by geometry and temporal-history adapters. */
  get identity(): SceneNode { return this; }
  get parent(): SceneNode | null { return this.parentNode; }
  get children(): ReadonlySet<SceneNode> { return this.childNodes; }
  get removed(): boolean { return this.invalidated; }

  /** Current own-transform revision, including direct local array edits. */
  get transformRevision(): number {
    this.assertAlive();
    this.updateLocalTransform();
    return this.ownRevision;
  }

  get worldRevision(): number {
    this.refreshWorld();
    return this.composedRevision;
  }

  /** Current world affine transform; translation is at indices 9–11. Removed nodes are invalid. */
  get worldTransform(): AffineTransform {
    this.refreshWorld();
    return this.affine;
  }

  get worldVisible(): boolean {
    this.refreshWorld();
    return this.inheritedVisible;
  }

  /** Copy local settings. Failed parent validation leaves all settings and hierarchy unchanged. */
  set(options: SceneNodeOptions) {
    this.setNodeOptions(options);
    return this;
  }

  /** Reparent within the owning scene, preserving local transforms; reject cycles atomically. */
  setParent(parent: SceneNode | null) {
    this.validateParent(parent);
    if(parent === this.parentNode) {
      return this;
    }
    this.parentNode?.childNodes.delete(this);
    this.parentNode = parent;
    parent?.childNodes.add(this);
    this.scene?.updateRootNode(this);
    return this;
  }

  /** @internal Update after the parent during the scene's single hierarchy traversal. */
  updateWorldTransform() {
    this.assertAlive();
    this.updateLocalTransform();
    const parent = this.parentNode;
    const parentRevision = parent?.composedRevision ?? 0;
    if(this.preparedOwnRevision !== this.ownRevision || this.preparedParent !== parent || this.parentRevision !== parentRevision) {
      const local = meshTransform(this);
      this.affine = Object.freeze(parent ? composeTransforms(parent.affine, local) : local);
      this.preparedOwnRevision = this.ownRevision;
      this.preparedParent = parent;
      this.parentRevision = parentRevision;
      this.composedRevision++;
    }
    this.inheritedVisible = this.visible && (parent?.inheritedVisible ?? true);
  }

  /** @internal Read cached world data after the scene traversal, without walking ancestors again. */
  preparedWorldState() {
    return {worldTransform: this.affine, worldVisible: this.inheritedVisible, worldRevision: this.composedRevision};
  }

  /** @internal The scene removes descendants before releasing this node's hierarchy references. */
  invalidate() {
    this.parentNode?.childNodes.delete(this);
    this.parentNode = null;
    this.childNodes.clear();
    this.invalidated = true;
  }

  protected setNodeOptions(options: SceneNodeOptions) {
    this.assertAlive();
    for(const name of TRANSFORM_FIELDS) {
      const value = options[name];
      if(value && (value.length !== 3 || value.some(component => !Number.isFinite(component)))) {
        throw new RangeError(`Node ${name} needs three finite values.`);
      }
    }
    if(options.parent !== undefined) {
      this.validateParent(options.parent);
    }
    for(const name of TRANSFORM_FIELDS) {
      const value = options[name];
      if(value) {
        this[name] = Array.from(value);
      }
    }
    this.visible = options.visible ?? this.visible;
    if(options.parent !== undefined) {
      this.setParent(options.parent);
    }
  }

  private validateParent(parent: SceneNode | null) {
    this.assertAlive();
    if(parent?.removed) {
      throw new Error('Cannot parent to a removed scene node.');
    }
    if(parent && (!this.scene || parent.scene !== this.scene || !this.scene.nodes.has(parent))) {
      throw new Error('Parent must belong to the same scene.');
    }
    for(let ancestor = parent; ancestor; ancestor = ancestor.parentNode) {
      if(ancestor === this) {
        throw new Error('Scene parenting cannot create a cycle.');
      }
    }
  }

  private updateLocalTransform() {
    const signature = [...this.position, ...this.rotation, ...this.scale];
    if(this.position.length !== 3 || this.rotation.length !== 3 || this.scale.length !== 3 || !signature.every(Number.isFinite)) {
      throw new RangeError('Scene node needs finite XYZ transforms.');
    }
    if(signature.every((value, index) => value === this.localSignature[index])) {
      return false;
    }
    this.localSignature = signature;
    this.ownRevision++;
    return true;
  }

  private refreshWorld() {
    // Public access updates the ancestor chain once, never once per transformed vertex.
    const ancestors: SceneNode[] = [];
    for(let node: SceneNode | null = this; node; node = node.parentNode) {
      ancestors.push(node);
    }
    for(let index = ancestors.length - 1; index >= 0; index--) {
      ancestors[index].updateWorldTransform();
    }
  }

  private assertAlive() {
    if(this.invalidated) {
      throw new Error('Scene node is removed.');
    }
    this.scene?.assertAlive();
  }
}
