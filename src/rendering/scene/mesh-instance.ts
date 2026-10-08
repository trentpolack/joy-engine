// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { MeshInstanceOptions } from '../../core/types.ts';
import type { MeshAsset } from './mesh-asset.ts';

/** Mutable presentation state borrowing an immutable MeshAsset. No simulation state. */
export class MeshInstance {
  declare asset: MeshAsset;
  declare position: number[];
  declare rotation: number[];
  declare scale: number[];
  declare tint: number[];
  declare opacity: number;
  declare visible: boolean;
  declare sceneRoughness: number;
  declare pass: "auto" | "opaque" | "transparent";

  /** @param asset
   * @param [options]
   */
  constructor(asset: MeshAsset, options: MeshInstanceOptions = {}) {
    this.asset = asset;
    this.position = [0, 0, 0];
    this.rotation = [0, 0, 0];
    this.scale = [1, 1, 1];
    this.tint = [1, 1, 1];
    this.opacity = 1;
    this.visible = true;
    this.sceneRoughness = 1;

    this.pass = 'auto';
    this.set(options);
  }

  /** Copy supplied settings; omitted fields retain their previous value.
   * Distances are world units; rotations use XYZ Euler radians. Zero scale is supported.
   * @param options
   */
  set(options: MeshInstanceOptions) {
    for(const name of  ((['position', 'rotation', 'scale', 'tint']) as const)) {
      const value = options[name];
      if(value && (value.length !== 3 || value.some(component => !Number.isFinite(component)))) {
        throw new RangeError(`Instance ${name} needs three finite values.`);
      }
    }
    if(options.opacity !== undefined && (!Number.isFinite(options.opacity) || options.opacity < 0 || options.opacity > 1)) {
      throw new RangeError('Instance opacity must be between zero and one.');
    }
    if(options.sceneRoughness !== undefined && (!Number.isFinite(options.sceneRoughness) || options.sceneRoughness < 0 || options.sceneRoughness > 1)) {
      throw new RangeError('Scene roughness must be between zero and one.');
    }
    if(options.pass !== undefined && !['auto', 'opaque', 'transparent'].includes(options.pass)) {
      throw new RangeError('Instance pass must be auto, opaque or transparent.');
    }
    for(const name of  ((['position', 'rotation', 'scale', 'tint']) as const)) {
      const value = options[name];
      if(value) {
        this[name] = Array.from(value);
      }
    }
    this.opacity = options.opacity ?? this.opacity;
    this.visible = options.visible ?? this.visible;
    this.pass = options.pass ?? this.pass;
    this.sceneRoughness = options.sceneRoughness ?? this.sceneRoughness;
    return this;
  }
}
