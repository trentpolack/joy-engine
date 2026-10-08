// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { PRIMITIVE_SHAPE } from '../rendering/geometry/constants.ts';
import type { EntityDefinition } from './types.ts';
import type { MeshInstance } from '../rendering/scene/mesh-instance.ts';
import { meshBox, meshSphere } from '../form/resources.ts';
import { MeshAsset } from '../rendering/scene/mesh-asset.ts';
import { RenderScene } from '../rendering/scene/render-scene.ts';

/**
 * Shared retained placeholder geometry for authored levels and runtime previews.
 * Owns its scene and instances. Transform scale multiplies render.size (full dimensions).
 * Components with no render record are invisible; unknown records remain uninterpreted.
 */
export class LevelScene {
  declare resolveMesh: (entity: EntityDefinition) => MeshAsset | undefined;
  declare scene: RenderScene;
  declare assets: { box: MeshAsset; sphere: MeshAsset; };
  private declare instances: Map<string, MeshInstance>;

  /**
   * Allocate a retained scene and reusable placeholder assets for this preview. */
  /** @param [resolveMesh] */
  constructor(resolveMesh: (entity: EntityDefinition) => MeshAsset | undefined = () => undefined) {
    this.resolveMesh = resolveMesh;
    this.scene = new RenderScene();
    this.assets = {
      box: new MeshAsset(meshBox(1, 1, 1)),
      sphere: new MeshAsset(meshSphere(0.5, 16))
    };
    /** @private */
    this.instances = new Map();
  }

  /**
   * Sync borrowed world-space definitions; copies presentation arrays, never mutates data.
   * @param definitions
   * @param [selectedId]
   */
  sync(definitions: readonly EntityDefinition[], selectedId: string | null = null) {
    const live = new Set();
    for(const entity of definitions) {
      const render = entity.components.render;
      const resolved = this.resolveMesh(entity);
      if(!render || (!resolved && render.shape !== PRIMITIVE_SHAPE.BOX && render.shape !== PRIMITIVE_SHAPE.SPHERE)) {
        continue;
      }
      live.add(entity.id);
      const asset = resolved ?? this.assets[ ((render.shape) as typeof PRIMITIVE_SHAPE.BOX | typeof PRIMITIVE_SHAPE.SPHERE)];
      let instance = this.instances.get(entity.id);
      if(instance?.asset !== asset) {
        if(instance) {
          this.scene.remove(instance);
        }
        instance = this.scene.createMesh(asset);
        this.instances.set(entity.id, instance);
      }
      const size = vector(render.size, [1, 1, 1]);
      const color = vector(render.color, resolved ? [1, 1, 1] : [0.95, 0.36, 0.12]);
      instance.set({
        ...entity.transform,
        scale: entity.transform.scale.map((value, axis) => value*size[axis]),
        tint: entity.id === selectedId ? color.map(value => Math.min(1, value*0.65 + 0.35)) : color
      });
    }
    for(const [id, instance] of this.instances) {
      if(!live.has(id)) {
        this.scene.remove(instance);
        this.instances.delete(id);
      }
    }
  }

  /**
   * Release retained scene instances and their owned scene resources. */
  destroy() {
    this.instances.clear();
    this.scene.destroy();
  }
}

/**
 * Read a finite presentation vector, borrowing the fallback when data is absent or invalid.
 * @param value
 * @param fallback
 */
function vector(value: unknown, fallback: number[]) {
  return Array.isArray(value) && value.length === 3 && value.every(item => typeof item === 'number' && Number.isFinite(item))
    ? value
    : fallback;
}
