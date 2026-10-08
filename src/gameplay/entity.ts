// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { EntityOptions, JsonRecord, EntityTransform } from './types.ts';
import type { EntityWorld } from './entity-world.ts';

import {copyEntityDefinition} from './entity-definition.ts';
import {EntityComponent} from './component.ts';

/** Instance-owned runtime data. Subclasses acquire resources in onSpawn and release them in onDestroy. */
export class Entity {
  declare id: string;
  declare name: string;
  declare type: string;
  declare tags: string[];
  declare transform: EntityTransform;
  declare components: Record<string, JsonRecord>;
  declare runtimeComponents: ReadonlyMap<string, EntityComponent>;
  declare world: EntityWorld | null;
  declare destroyed: boolean;
  private declare resources: { destroy: () => void; }[];
  private declare mutableRuntimeComponents: Map<string, EntityComponent>;

  /**
   * @param definition
   */
  constructor(definition: EntityOptions) {
    const data = copyEntityDefinition(definition);
    /** Stable world lookup key; do not mutate during this lifetime. @readonly */
    this.id = data.id;
    this.name = data.name;
    this.type = data.type;
    this.tags = data.tags;
    this.transform = data.transform;
    this.components = data.components;
    this.mutableRuntimeComponents = new Map();
    this.runtimeComponents = this.mutableRuntimeComponents;

    /** Borrowed owner; set only by EntityWorld. */
    this.world = null;
    this.destroyed = false;
    /** Owned resources, released in reverse acquisition order. @private */
    this.resources = [];
  }

  /**
   * Acquire subclass resources after registration. Throwing triggers onDestroy rollback.
   * Factories should construct data only: a constructor that throws must clean up its own resources.
   * @param _world Borrowed owning world.
   */
  onSpawn(_world: EntityWorld) {}

  /**
   * Subclass simulation hook; the base entity has no update behavior.
   * @param _elapsedSeconds Nonnegative simulation seconds, supplied by the world.
   */
  update(_elapsedSeconds: number) {}

  /**
   * Attach owned runtime behavior. Authored data remains available through components.
   * @param component Fresh unowned component.
   */
  addComponent<T extends EntityComponent>(component: T): T {
    if(this.destroyed) {
      throw new Error('Cannot attach a component to a destroyed entity.');
    }
    if(!(component instanceof EntityComponent) || component.entity || component.destroyed) {
      throw new Error('Entity components must be fresh and unowned.');
    }
    if(this.mutableRuntimeComponents.has(component.type)) {
      throw new Error(`Entity ${this.id} already has component ${component.type}.`);
    }

    component.entity = this;
    this.mutableRuntimeComponents.set(component.type, component);
    try {
      component.onAttach(this);
    } catch(error) {
      this.mutableRuntimeComponents.delete(component.type);
      component.entity = null;
      try {
        component.destroy();
      } catch(cleanupError) {
        throw new AggregateError([error, cleanupError], `Component ${component.type} attachment and cleanup failed.`);
      }
      throw error;
    }
    return component;
  }

  /** Return borrowed runtime behavior for a component type, or null. */
  getComponent<T extends EntityComponent = EntityComponent>(type: string): T | null {
    return (this.mutableRuntimeComponents.get(type) as T | undefined) ?? null;
  }

  /** Detach and destroy owned runtime behavior. */
  removeComponent(type: string): boolean {
    const component = this.mutableRuntimeComponents.get(type);
    if(!component) {
      return false;
    }
    this.mutableRuntimeComponents.delete(type);
    component.entity = null;
    component.destroy();
    return true;
  }

  /**
   * Transfer resource cleanup to this entity; returns the same resource for convenient setup.
   * Acquire dependencies before dependents so reverse disposal preserves their lifetime order.
   * @param resource
   */
  own<T extends {
    destroy: () => void;
}>(resource: T): T {
    if(this.destroyed) {
      throw new Error('Cannot attach resources to a destroyed entity.');
    }
    if(!resource || typeof resource.destroy !== 'function') {
      throw new TypeError('Entity resources must provide destroy().');
    }
    if(!this.resources.includes(resource)) {
      this.resources.push(resource);
    }
    return resource;
  }

  /** Remove this instance from its world and release resources exactly once, even if cleanup throws. */
  destroy() {
    if(this.destroyed) {
      return;
    }
    if(this.world) {
      this.world.remove(this.id);
      return;
    }
    this.destroyed = true;
    const errors = [];
    try {
      this.onDestroy();
    } catch(error) {
      errors.push(error);
    }
    for(const componentType of [...this.mutableRuntimeComponents.keys()].reverse()) {
      try {
        this.removeComponent(componentType);
      } catch(error) {
        errors.push(error);
      }
    }
    for(const resource of this.resources.splice(0).reverse()) {
      try {
        resource.destroy();
      } catch(error) {
        errors.push(error);
      }
    }
    if(errors.length) {
      throw new AggregateError(errors, 'Entity cleanup failed.');
    }
  }

  /** Release subclass resources. Called even after partially completed onSpawn. */
  onDestroy() {}
}
