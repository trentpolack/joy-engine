// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {Entity} from './entity.ts';
import type {JsonRecord} from './types.ts';
import {EntityComponent} from './component.ts';

export type ComponentFactory = (data: JsonRecord, entity: Entity) => EntityComponent;

/** Maps trusted component names to runtime behavior without evaluating authored data. */
export class ComponentRegistry {
  private declare factories: Map<string, ComponentFactory>;

  constructor() {
    this.factories = new Map();
  }

  /**
   * Register project behavior for an authored component type.
   * @param type Nonempty component type.
   * @param factory Borrowed factory; the returned component is owned by its entity.
   */
  register(type: string, factory: ComponentFactory): this {
    if(typeof type !== 'string' || !type.trim() || typeof factory !== 'function') {
      throw new TypeError('Component registration requires a nonempty type and factory.');
    }
    if(this.factories.has(type)) {
      throw new Error(`Component type ${type} is already registered.`);
    }
    this.factories.set(type, factory);
    return this;
  }

  /** Return whether this registry can materialize runtime behavior for an authored type. */
  has(type: string): boolean {
    return this.factories.has(type);
  }

  /**
   * Construct an unowned runtime component over the entity's owned authored record.
   * @param type Registered component type.
   * @param data Entity-owned mutable component data.
   * @param entity Borrowed target entity.
   */
  create(type: string, data: JsonRecord, entity: Entity): EntityComponent {
    const factory = this.factories.get(type);
    if(!factory) {
      throw new Error(`Unknown component type: ${type}.`);
    }
    const component = factory(data, entity);
    if(!(component instanceof EntityComponent)) {
      throw new TypeError('Component factory must return an EntityComponent.');
    }
    if(component.type !== type) {
      try {
        component.destroy();
      } catch(error) {
        throw new AggregateError([error], `Component factory for ${type} returned the wrong type and cleanup failed.`);
      }
      throw new Error(`Component factory for ${type} must return the matching component type.`);
    }
    if(component.entity || component.destroyed) {
      throw new Error('Component factory must return a fresh, unowned component.');
    }
    return component;
  }
}
