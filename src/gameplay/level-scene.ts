// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { PRIMITIVE_SHAPE } from '../core/constants.ts';
import { COMPONENT_TYPE } from './constants.ts';
import { LIGHT_COMPONENT_DEFAULTS, RENDER_COMPONENT_DEFAULTS } from './built-in-component-schemas.ts';
import type { EntityDefinition } from './types.ts';
import type { PointLight } from '../rendering/lighting/point-lights.ts';
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
  /** Copied world-space lights derived from authored light components. */
  declare lights: PointLight[];

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
    this.lights = [];
  }

  /**
   * Sync borrowed world-space definitions; copies presentation arrays, never mutates data.
   * @param definitions
   * @param [selectedId]
   */
  sync(definitions: readonly EntityDefinition[], selectedId: string | null = null) {
    const live = new Set();
    const lights: PointLight[] = [];
    for(const entity of definitions) {
      const light = entity.components[COMPONENT_TYPE.LIGHT];
      if(light && light.enabled !== false) {
        const color = vector(light.color, LIGHT_COMPONENT_DEFAULTS.color);
        const intensity = finite(light.intensity, LIGHT_COMPONENT_DEFAULTS.intensity);
        const range = finite(light.range, LIGHT_COMPONENT_DEFAULTS.range);
        if(intensity < 0 || range <= 0 || color.some(value => value < 0)) {
          throw new RangeError(`Entity ${entity.id}: light color/intensity must be nonnegative and range must be positive.`);
        }
        lights.push({
          x: entity.transform.position[0],
          y: entity.transform.position[1],
          z: entity.transform.position[2],
          r: color[0]*intensity,
          g: color[1]*intensity,
          b: color[2]*intensity,
          range,
          castsShadow: light.castsShadow !== false
        });
      }
      const render = entity.components[COMPONENT_TYPE.RENDER];
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
      const size = vector(render.size, RENDER_COMPONENT_DEFAULTS.size);
      const color = vector(render.color, resolved ? RENDER_COMPONENT_DEFAULTS.meshTint : RENDER_COMPONENT_DEFAULTS.primitiveColor);
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
    this.lights = lights;
  }

  /**
   * Release retained scene instances and their owned scene resources. */
  destroy() {
    this.instances.clear();
    this.lights = [];
    this.scene.destroy();
  }
}

/** Read a finite scalar while retaining a clear default for omitted authored fields. */
function finite(value: unknown, fallback: number) {
  if(value === undefined) {
    return fallback;
  }
  if(typeof value !== 'number' || !Number.isFinite(value)) {
    throw new TypeError('Scene light values must be finite numbers.');
  }
  return value;
}

/**
 * Read a finite presentation vector, borrowing the fallback when data is absent or invalid.
 * @param value
 * @param fallback
 */
function vector(value: unknown, fallback: readonly number[]): readonly number[] {
  return Array.isArray(value) && value.length === 3 && value.every(item => typeof item === 'number' && Number.isFinite(item))
    ? value
    : fallback;
}
