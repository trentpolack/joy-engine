// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { PARTICLE_RENDERER } from '../particles/constants.ts';
import type { EntityDefinition } from './types.ts';
import type { ParticleEffectOptions } from '../particles/particle-effect.ts';
import type { ParticleCameraBasis } from '../particles/rendering.ts';
import type { ParameterValue } from '../form/parameters.ts';
import type { CompiledParticleEffect } from '../particles/asset.ts';
import type { RenderScene } from '../rendering/scene/render-scene.ts';
import { LevelScene } from './level-scene.ts';
import { copyEntityDefinition } from './entity-definition.ts';
import { resolveLevelObjects } from './object-document.ts';
import { assetPath } from './asset-path.ts';
import { compile } from '../form/compiler.ts';
import { MeshAsset } from '../rendering/scene/mesh-asset.ts';
import { ParticleEffect } from '../particles/particle-effect.ts';
import { compileParticleEffect } from '../particles/asset.ts';
import { appendParticleEffect } from '../particles/rendering.ts';

export type { EntityDefinition };
export interface EffectInstance {
  effect: ParticleEffect;
  key: string;
  loop: boolean;
}

export interface AssetSceneOptions {
  readText: (path: string) => Promise<string>;
}

/**
 * Own authored asset presentation, compiled caches and per-instance effect simulation.
 * readText is borrowed and must enforce its project boundary. sync stages all resources
 * before replacing the displayed result; update changes only live transforms.
 */
export class AssetScene {
  declare readText: (path: string) => Promise<string>;
  declare meshes: Map<string, MeshAsset>;
  declare effects: Map<string, EffectInstance>;
  declare meshCache: Map<string, MeshAsset>;
  declare effectCache: Map<string, CompiledParticleEffect>;
  declare entities: EntityDefinition[];
  declare level: LevelScene;
  declare scene: RenderScene;
  declare assets: { box: MeshAsset; sphere: MeshAsset; };
  declare generation: number;
  declare destroyed: boolean;
  declare error: string;

  /** @param options */
  constructor({readText}: AssetSceneOptions) {
    this.readText = readText;

    this.meshes = new Map();

    this.effects = new Map();

    this.meshCache = new Map();

    this.effectCache = new Map();

    this.entities = [];
    this.level = new LevelScene(entity => this.meshes.get(entity.id));
    this.scene = this.level.scene;
    this.assets = this.level.assets;
    this.generation = 0;
    this.destroyed = false;
    this.error = '';
  }

  /**
   * Resolve and prepare a replacement atomically. Returns false for a superseded load.
   * @param entities @param [selectedId]
   */
  async sync(entities: readonly EntityDefinition[], selectedId: string | null = null) {
    if(this.destroyed) {
      throw new Error('AssetScene is destroyed.');
    }
    const generation = ++this.generation;

    const reads: Map<string, Promise<string>> = new Map();
    const read = (path: string) => {
      assetPath(path);
      if(!reads.has(path)) {
        reads.set(path, this.readText(path));
      }
      return  ((reads.get(path)) as Promise<string>);
    };
    const resolved = await resolveLevelObjects({version: 1, name: 'Presentation', entities: entities.map(copyEntityDefinition)}, read);
    const meshes = new Map();

    const effects: Map<string, EffectInstance> = new Map();

    const created: ParticleEffect[] = [];

    const meshCache: Map<string, MeshAsset> = new Map();

    const effectCache: Map<string, CompiledParticleEffect> = new Map();
    try {
      for(const entity of resolved.entities) {
        const render = entity.components.render;
        if(render?.asset) {
          const path = assetPath(render.asset, ['.form', '.formlab']);
          const text = await read(path);
          const key = JSON.stringify([path, text, render.parameters ?? {}]);
          let mesh = meshCache.get(key) ?? this.meshCache.get(key);
          if(!mesh) {
            const document = path.endsWith('.formlab') ? JSON.parse(text) : {source: text};
            if(path.endsWith('.formlab') && document.version !== 1) {
              throw new Error('Unsupported FORM LAB document version.');
            }
            if(typeof document.source !== 'string') {
              throw new Error('FORM LAB document requires script source.');
            }
            const overrides = {...document.overrides, ...parameterRecord(render.parameters)};
            const result = compile(document.source, overrides, document.materials);
            mesh = new MeshAsset({positions: result.positions, indices: result.triangles, normals: result.normals,
              colors: result.colors, alphas: result.alphas, uvs: result.uvs, materials: result.materials,
              triangleMaterials: result.triangleMaterials, metadata: {parameters: result.parameters}});
          }
          meshCache.set(key, mesh);
          meshes.set(entity.id, mesh);
        }
        const settings = entity.components.effect;
        if(settings?.asset && settings.enabled !== false) {
          const path = assetPath(settings.asset, ['.joyfx']);
          const text = await read(path);
          let definition = effectCache.get(text) ?? this.effectCache.get(text);
          if(!definition) {
            definition = compileParticleEffect(JSON.parse(text));
          }
          if(definition.emitters.some(emitter => emitter.renderer === PARTICLE_RENDERER.TEXTURED || emitter.renderer === PARTICLE_RENDERER.FLIPBOOK)) {
            throw new Error(`${path}: textured effects need a texture-capable scene renderer. Open PARTICLE LAB to preview them.`);
          }
          effectCache.set(text, definition);
          const key = JSON.stringify([text, settings]);
          const previous = this.effects.get(entity.id);
          if(previous?.key === key) {
            effects.set(entity.id, previous);
          } else {
            const seed = settings.seed === undefined ? 7777 : Number(settings.seed);
            const effect = new ParticleEffect(definition, {seed, parameters:  ((parameterRecord(settings.parameters)) as ParticleEffectOptions['parameters'])});
            created.push(effect);
            effect.step();
            effects.set(entity.id, {effect, key, loop: settings.loop === true});
          }
        }
      }
      if(this.destroyed || generation !== this.generation) {
        return false;
      }
      for(const [id, previous] of this.effects) {
        if(effects.get(id) !== previous) {
          previous.effect.destroy();
        }
      }
      this.meshes = meshes;
      this.effects = effects;
      this.meshCache = meshCache;
      this.effectCache = effectCache;
      this.error = '';
      this.update(resolved.entities, selectedId);
      created.length = 0;
      return true;
    } finally {
      for(const effect of created) {
        effect.destroy();
      }
    }
  }

  /**
   * Invalidate pending loads while retaining the current scene. Call when a newer
   * owner transaction starts before its own asset reads are ready.
   */
  cancelPending() {
    this.generation++;
  }

  /**
   * Copy current transforms without reading assets or resetting effect time.
   * @param entities @param [selectedId]
   */
  update(entities: readonly EntityDefinition[], selectedId: string | null = null) {
    if(this.destroyed) {
      return;
    }
    this.entities = entities.map(copyEntityDefinition);
    this.level.sync(this.entities, selectedId);
  }

  /**
   * Advance effects in seconds, clamping background stalls and isolating script failures.
   * @param seconds
   */
  step(seconds: number) {
    if(this.destroyed || !Number.isFinite(seconds) || seconds < 0) {
      return;
    }
    for(const {effect, loop} of this.effects.values()) {
      if(effect.error) {
        continue;
      }
      try {
        if(loop && !effect.isAlive) {
          effect.reset();
        }
        effect.update(Math.min(0.1, seconds));
      } catch(error) {
        this.error = error instanceof Error ? error.message : String(error);
      }
    }
  }

  /**
   * Build camera-facing colored effects attached to each entity's transform.
   * @param camera */
  particles(camera: ParticleCameraBasis): number[] {
    const vertices = [];
    for(const entity of this.entities) {
      const runtime = this.effects.get(entity.id);
      if(!runtime) {
        continue;
      }

      const local: number[] = [];
      // Convert the camera into the local rotated frame so attached particles remain billboards.
      const basis = {
        right: rotate(camera.right, entity.transform.rotation, true),
        up: rotate(camera.up, entity.transform.rotation, true),
        backward: rotate(camera.backward, entity.transform.rotation, true)
      };
      appendParticleEffect(local, runtime.effect, basis);
      for(let offset = 0; offset < local.length; offset+= 7) {
        const point = rotate(local.slice(offset, offset + 3).map((value, axis) => value*entity.transform.scale[axis]), entity.transform.rotation);
        vertices.push(...point.map((value, axis) => value + entity.transform.position[axis]), ...local.slice(offset + 3, offset + 7));
      }
    }
    return vertices;
  }

  /**
   * Dispose every effect and invalidate in-flight reads before releasing scene references. */
  destroy() {
    if(this.destroyed) {
      return;
    }
    this.destroyed = true;
    this.generation++;
    for(const {effect} of this.effects.values()) {
      effect.destroy();
    }
    this.effects.clear();
    this.meshes.clear();
    this.meshCache.clear();
    this.effectCache.clear();
    this.level.destroy();
  }
}

/**
 * Read component parameter overrides as plain records; compilers validate their values.
 * @param value */
function parameterRecord(value: unknown): Record<string, ParameterValue> {
  if(value === undefined) {
    return {};
  }
  if(!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new TypeError('Asset parameters must be a record.');
  }
  return  ((value) as Record<string, ParameterValue>);
}

/**
 * Rotate a direction using the engine's XYZ Euler convention or its inverse.
 * @param point @param angles @param [inverse]
 */
function rotate(point: readonly number[], angles: readonly number[], inverse: boolean = false) {
  let [x, y, z] = point;
  const axes = inverse ? [2, 1, 0] : [0, 1, 2];
  for(const axis of axes) {
    const angle = angles[axis]*(inverse ? -1 : 1);
    const c = Math.cos(angle), s = Math.sin(angle);
    if(axis === 0) {
      [y, z] = [y*c - z*s, y*s + z*c];
    } else if(axis === 1) {
      [x, z] = [x*c + z*s, -x*s + z*c];
    } else {
      [x, y] = [x*c - y*s, x*s + y*c];
    }
  }
  return [x, y, z];
}
