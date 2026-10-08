// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {Entity} from './entity.ts';
import type {EntityWorld} from './entity-world.ts';

export interface EntitySystemOptions {
  requiredComponents?: readonly string[];
}

/** World-owned processor for entities selected by runtime component composition. */
export class EntitySystem {
  declare readonly requiredComponents: readonly string[];
  declare world: EntityWorld | null;
  declare destroyed: boolean;

  /**
   * @param options Query constraints applied before every update.
   */
  constructor({requiredComponents = []}: EntitySystemOptions = {}) {
    if(!Array.isArray(requiredComponents) || requiredComponents.some(type => typeof type !== 'string' || !type.trim())) {
      throw new TypeError('System component requirements must be nonempty strings.');
    }
    this.requiredComponents = Object.freeze([...new Set(requiredComponents)]);
    this.world = null;
    this.destroyed = false;
  }

  /** Acquire system-wide resources after the world takes ownership. */
  onAdd(_world: EntityWorld) {}

  /** Process a stable insertion-order snapshot. Elapsed time is finite nonnegative seconds. */
  update(_elapsedSeconds: number, _entities: readonly Entity[]) {}

  /** Release system-wide resources exactly once. */
  onRemove() {}

  /** Remove this system from its world, or finish an unowned system lifetime. */
  destroy() {
    if(this.destroyed) {
      return;
    }
    if(this.world) {
      this.world.removeSystem(this);
      return;
    }
    this.destroyed = true;
    this.onRemove();
  }
}
