// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { EntityOptions } from './types.ts';
import type { Entity } from './entity.ts';

import {EntityRegistry} from './entity-registry.ts';
import {ComponentRegistry} from './component-registry.ts';
import {EntitySystem} from './entity-system.ts';

export interface EntityWorldOptions {
  registry?: EntityRegistry;
  componentRegistry?: ComponentRegistry;
}

/** Owns entities and deterministic insertion-order updates, independently of rendering and controllers. */
export class EntityWorld {
  declare registry: EntityRegistry;
  declare entities: Map<string, Entity>;
  declare componentRegistry: ComponentRegistry;
  declare systems: readonly EntitySystem[];
  declare destroyed: boolean;
  private declare spawning: Set<string>;
  private declare mutableSystems: EntitySystem[];

  /**
   * @param [options]
   */
  constructor({registry = new EntityRegistry(), componentRegistry = new ComponentRegistry()}: EntityWorldOptions = {}) {
    this.registry = registry;
    this.componentRegistry = componentRegistry;
    /** Borrowed view for inspection; mutate ownership through spawn/remove only. */
    this.entities = new Map();
    this.mutableSystems = [];
    this.systems = this.mutableSystems;
    this.destroyed = false;
    /** Reserve ids during reentrant factory calls. @private */
    this.spawning = new Set();
  }

  /**
   * Create and own an entity. Failed setup removes registration and invokes cleanup before throwing.
   * @param definition Copied authored input.
   * @returns Borrowed runtime instance.
   */
  spawn(definition: EntityOptions): Entity {
    if(this.destroyed) {
      throw new Error('Cannot spawn into a destroyed world.');
    }
    if(this.entities.has(definition.id) || this.spawning.has(definition.id)) {
      throw new Error(`Entity id ${definition.id} is already present.`);
    }
    const id = definition.id;
    this.spawning.add(id);

    let entity: Entity | null = null;
    try {
      entity = this.registry.create(definition);
      if(this.destroyed) {
        throw new Error('World was destroyed during entity creation.');
      }
      entity.world = this;
      this.entities.set(id, entity);
      for(const [type, data] of Object.entries(entity.components)) {
        if(this.componentRegistry.has(type)) {
          entity.addComponent(this.componentRegistry.create(type, data, entity));
        }
      }
      entity.onSpawn(this);
      if(entity.destroyed || entity.world !== this || this.destroyed) {
        throw new Error('Entity was destroyed during spawn.');
      }
      return entity;
    } catch(error) {
      if(entity) {
        if(this.entities.get(id) === entity) {
          this.entities.delete(id);
        }
        entity.world = null;
        try {
          entity.destroy();
        } catch(cleanupError) {
          throw new AggregateError([error, cleanupError], 'Entity spawn and cleanup failed.');
        }
      }
      throw error;
    } finally {
      this.spawning.delete(id);
    }
  }

  /**
   * Add an owned system at the end of deterministic update order.
   * @param system Fresh unowned system.
   */
  addSystem<T extends EntitySystem>(system: T): T {
    if(this.destroyed) {
      throw new Error('Cannot add a system to a destroyed world.');
    }
    if(!(system instanceof EntitySystem) || system.world || system.destroyed || this.mutableSystems.includes(system)) {
      throw new Error('World systems must be fresh and unowned.');
    }
    system.world = this;
    this.mutableSystems.push(system);
    try {
      system.onAdd(this);
    } catch(error) {
      this.mutableSystems.pop();
      system.world = null;
      try {
        system.destroy();
      } catch(cleanupError) {
        throw new AggregateError([error, cleanupError], 'System setup and cleanup failed.');
      }
      throw error;
    }
    return system;
  }

  /** Remove and destroy an owned system. */
  removeSystem(system: EntitySystem): boolean {
    const index = this.mutableSystems.indexOf(system);
    if(index < 0) {
      return false;
    }
    this.mutableSystems.splice(index, 1);
    system.world = null;
    system.destroy();
    return true;
  }

  /** Return a stable insertion-order snapshot matching every runtime component type. */
  query(componentTypes: readonly string[]): Entity[] {
    if(!Array.isArray(componentTypes) || componentTypes.some(type => typeof type !== 'string' || !type.trim())) {
      throw new TypeError('Entity queries require component type strings.');
    }
    return [...this.entities.values()].filter(entity => (
      !entity.destroyed && componentTypes.every(type => entity.runtimeComponents.has(type))
    ));
  }

  /**
   * Look up a live registration without transferring ownership.
   * @param id
   * @returns Borrowed instance, or null.
   */
  get(id: string): Entity | null {
    return this.entities.get(id) ?? null;
  }

  /**
   * Remove ownership before cleanup, so reentrant callbacks never observe a half-destroyed entity.
   * Cleanup failures propagate after the entity is detached.
   * @param id
   * @returns Whether a registered entity was removed.
   */
  remove(id: string): boolean {
    const entity = this.entities.get(id);
    if(!entity) {
      return false;
    }
    this.entities.delete(id);
    entity.world = null;
    entity.destroy();
    return true;
  }

  /**
   * Update surviving entities in insertion order. New spawns wait for the next update.
   * @param elapsedSeconds Finite nonnegative simulation seconds.
   */
  update(elapsedSeconds: number) {
    if(!Number.isFinite(elapsedSeconds) || elapsedSeconds < 0) {
      throw new RangeError('World delta must be finite nonnegative seconds.');
    }
    if(this.destroyed) {
      return;
    }
    const frameEntities = [...this.entities.values()];
    for(const system of [...this.mutableSystems]) {
      if(system.world === this && !system.destroyed) {
        const matches = frameEntities.filter(entity => (
          this.entities.get(entity.id) === entity
          && !entity.destroyed
          && system.requiredComponents.every(type => entity.runtimeComponents.has(type))
        ));
        system.update(elapsedSeconds, matches);
      }
    }
    for(const entity of frameEntities) {
      if(this.entities.get(entity.id) === entity && !entity.destroyed) {
        entity.update(elapsedSeconds);
      }
    }
  }

  /** Dispose every entity even if individual cleanup fails; report all errors after releasing ownership. */
  destroy() {
    if(this.destroyed) {
      return;
    }
    this.destroyed = true;
    const errors = [];
    for(const system of [...this.mutableSystems].reverse()) {
      try {
        this.removeSystem(system);
      } catch(error) {
        errors.push(error);
      }
    }
    for(const id of [...this.entities.keys()]) {
      try {
        this.remove(id);
      } catch(error) {
        errors.push(error);
      }
    }
    if(errors.length) {
      throw new AggregateError(errors, 'Entity world cleanup failed.');
    }
  }
}
