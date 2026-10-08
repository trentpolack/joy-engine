// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {Entity} from './entity.ts';
import type {EntityWorld} from './entity-world.ts';
import type {JsonRecord} from './types.ts';

/** Runtime behavior attached to one entity. Authored component data remains a plain JSON record. */
export class EntityComponent<TData extends JsonRecord = JsonRecord> {
  declare readonly type: string;
  declare readonly data: TData;
  declare entity: Entity | null;
  declare destroyed: boolean;
  private declare resources: {destroy: () => void;}[];

  /**
   * Construct an unowned component. EntityWorld attaches registered components during spawn.
   * @param type Stable component type used by queries and registries.
   * @param data Entity-owned authored data; factories may retain and mutate this record.
   */
  constructor(type: string, data: TData) {
    if(typeof type !== 'string' || !type.trim()) {
      throw new TypeError('Component type must be a nonempty string.');
    }
    if(!data || typeof data !== 'object' || Array.isArray(data)) {
      throw new TypeError('Component data must be a JSON record.');
    }

    this.type = type;
    this.data = data;
    this.entity = null;
    this.destroyed = false;
    this.resources = [];
  }

  /** Borrow the current world, or null before attachment and after removal. */
  get world(): EntityWorld | null {
    return this.entity?.world ?? null;
  }

  /** Acquire resources after attachment. Throwing rolls the component back and releases them. */
  onAttach(_entity: Entity) {}

  /** Release subclass state. The base path invokes this exactly once. */
  onDetach() {}

  /**
   * Transfer resource cleanup to the component.
   * @param resource Owned disposable, released in reverse acquisition order.
   */
  own<T extends {destroy: () => void;}>(resource: T): T {
    if(this.destroyed) {
      throw new Error('Cannot attach resources to a destroyed component.');
    }
    if(!resource || typeof resource.destroy !== 'function') {
      throw new TypeError('Component resources must provide destroy().');
    }
    if(!this.resources.includes(resource)) {
      this.resources.push(resource);
    }
    return resource;
  }

  /** Detach from the owning entity and release resources exactly once. */
  destroy() {
    if(this.destroyed) {
      return;
    }
    if(this.entity) {
      this.entity.removeComponent(this.type);
      return;
    }
    this.destroyed = true;

    const errors = [];
    try {
      this.onDetach();
    } catch(error) {
      errors.push(error);
    }
    for(const resource of this.resources.splice(0).reverse()) {
      try {
        resource.destroy();
      } catch(error) {
        errors.push(error);
      }
    }
    if(errors.length) {
      throw new AggregateError(errors, `Component ${this.type} cleanup failed.`);
    }
  }
}
