// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { MeshInstanceOptions } from '../../core/types.ts';
import type { MeshAsset } from './mesh-asset.ts';
import type { RenderScene } from './render-scene.ts';

import { SceneNode } from './scene-node.ts';

export const MESH_PASS = {AUTO: 'auto', OPAQUE: 'opaque', TRANSPARENT: 'transparent'} as const;

/** Geometry-bearing node borrowing an immutable MeshAsset. No simulation or GPU state. */
export class MeshInstance extends SceneNode {
  asset: MeshAsset;
  tint = [1, 1, 1];
  opacity = 1;
  sceneRoughness = 1;
  pass: 'auto' | 'opaque' | 'transparent' = MESH_PASS.AUTO;
  castsShadow = true;
  receivesShadow = true;

  constructor(asset: MeshAsset, options: MeshInstanceOptions = {}, scene: RenderScene | null = null) {
    validateMeshOptions(options);
    super(options, scene);
    this.asset = asset;
    this.setMeshOptions(options);
  }

  /** Copy supplied local and presentation settings; omitted fields retain their previous values. */
  override set(options: MeshInstanceOptions) {
    validateMeshOptions(options);
    this.setNodeOptions(options);
    this.setMeshOptions(options);
    return this;
  }

  private setMeshOptions(options: MeshInstanceOptions) {
    if(options.tint) {
      this.tint = Array.from(options.tint);
    }
    this.opacity = options.opacity ?? this.opacity;
    this.pass = options.pass ?? this.pass;
    this.sceneRoughness = options.sceneRoughness ?? this.sceneRoughness;
    this.castsShadow = options.castsShadow ?? this.castsShadow;
    this.receivesShadow = options.receivesShadow ?? this.receivesShadow;
  }
}

function validateMeshOptions(options: MeshInstanceOptions) {
  if(options.tint && (options.tint.length !== 3 || options.tint.some(component => !Number.isFinite(component)))) {
    throw new RangeError('Instance tint needs three finite values.');
  }
  if(options.opacity !== undefined && (!Number.isFinite(options.opacity) || options.opacity < 0 || options.opacity > 1)) {
    throw new RangeError('Instance opacity must be between zero and one.');
  }
  if(options.sceneRoughness !== undefined && (!Number.isFinite(options.sceneRoughness) || options.sceneRoughness < 0 || options.sceneRoughness > 1)) {
    throw new RangeError('Scene roughness must be between zero and one.');
  }
  if(options.pass !== undefined && !Object.values(MESH_PASS).includes(options.pass)) {
    throw new RangeError('Instance pass must be auto, opaque, or transparent.');
  }
}
