// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { 
  GeometryBounds,
  TerrainInstanceOptions
} from '../../core/types.ts';
import type { TerrainAsset } from '../terrain/terrain-asset.ts';
import type { RenderScene } from './render-scene.ts';

import { SceneNode } from './scene-node.ts';

const TERRAIN_DEFAULTS = {
  TOLERANCE: 2,
  TARGET_RADIUS: 0,
  DIAGNOSTIC: 0
} as const;

/** Scene-owned resident geometry node. Authored heights and GPU resources remain borrowed. */
export class TerrainInstance extends SceneNode {
  readonly asset: TerrainAsset;
  readonly localBounds: GeometryBounds;
  castsShadow = true;
  receivesShadow = true;
  tolerance: number = TERRAIN_DEFAULTS.TOLERANCE;
  targetRadius: number = TERRAIN_DEFAULTS.TARGET_RADIUS;
  diagnostic: number = TERRAIN_DEFAULTS.DIAGNOSTIC;

  constructor(asset: TerrainAsset, options: TerrainInstanceOptions = {}, scene: RenderScene | null = null) {
    validateTerrainOptions(options);
    const bounds = terrainBounds(asset);
    super(options, scene);
    this.asset = asset;
    this.localBounds = bounds;
    this.setTerrainOptions(options);
  }

  /** Update local transforms and pass/LOD policy atomically, preserving borrowed geometry. */
  override set(options: TerrainInstanceOptions) {
    validateTerrainOptions(options);
    this.setNodeOptions(options);
    this.setTerrainOptions(options);
    return this;
  }

  private setTerrainOptions(options: TerrainInstanceOptions) {
    this.castsShadow = options.castsShadow ?? this.castsShadow;
    this.receivesShadow = options.receivesShadow ?? this.receivesShadow;
    this.tolerance = options.tolerance ?? this.tolerance;
    this.targetRadius = options.targetRadius ?? this.targetRadius;
    this.diagnostic = options.diagnostic ?? this.diagnostic;
  }
}

function validateTerrainOptions(options: TerrainInstanceOptions) {
  if(options.tolerance !== undefined && (!Number.isFinite(options.tolerance) || options.tolerance <= 0)) {
    throw new RangeError('Terrain tolerance must be finite and positive.');
  }
  if(options.targetRadius !== undefined && (!Number.isFinite(options.targetRadius) || options.targetRadius < 0)) {
    throw new RangeError('Terrain target radius must be finite and nonnegative.');
  }
  if(options.diagnostic !== undefined && (!Number.isFinite(options.diagnostic) || options.diagnostic < 0)) {
    throw new RangeError('Terrain diagnostic must be finite and nonnegative.');
  }
}

function terrainBounds(asset: TerrainAsset): GeometryBounds {
  let minimumHeight = Infinity;
  let maximumHeight = -Infinity;
  for(const patch of asset.patches) {
    minimumHeight = Math.min(minimumHeight, patch.min);
    maximumHeight = Math.max(maximumHeight, patch.max);
  }
  const [extentX, extentZ] = asset.manifest.extent;
  return Object.freeze({min: Object.freeze([-extentX/2, minimumHeight, -extentZ/2]),
    max: Object.freeze([extentX/2, maximumHeight, extentZ/2])});
}
