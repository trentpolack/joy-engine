// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {EntityWorldOptions} from './entity-world.ts';
import type {PhysicsWorldOptions} from '../physics/physics-world.ts';

import {EntityWorld} from './entity-world.ts';
import {PhysicsWorld} from '../physics/physics-world.ts';

export interface WorldOptions {
  entities?: EntityWorldOptions;
  physics?: PhysicsWorldOptions;
  /** Borrowed solver; when present, physics options are invalid and the caller retains ownership. */
  physicsWorld?: PhysicsWorld;
}

export interface WorldStep {
  readonly elapsedSeconds: number;
  readonly physicsSteps: number;
}

/**
 * Owns headless entity simulation and physics without importing browser or rendering facilities.
 * Physics advances before entity systems, allowing systems to consume the completed solver state.
 * A presentation integration may borrow this world, but the world never owns that integration.
 */
export class World {
  readonly entities: EntityWorld;
  readonly physics: PhysicsWorld;
  readonly ownsPhysics: boolean;
  destroyed = false;

  /** Create independent simulation owners. Options are borrowed and copied by each subsystem. */
  constructor(options: WorldOptions = {}) {
    if(options.physicsWorld && options.physics) {
      throw new Error('World accepts physics options or a borrowed physics world, not both.');
    }
    this.entities = new EntityWorld(options.entities);
    this.physics = options.physicsWorld ?? new PhysicsWorld(options.physics);
    this.ownsPhysics = !options.physicsWorld;
  }

  /**
   * Advance physics and entities by finite, nonnegative simulation seconds.
   * @param elapsedSeconds Simulation delta in seconds.
   * @returns An owned immutable summary of completed work.
   */
  update(elapsedSeconds: number): WorldStep {
    if(this.destroyed) {
      throw new Error('World has been destroyed.');
    }
    if(!Number.isFinite(elapsedSeconds) || elapsedSeconds < 0) {
      throw new RangeError('World delta must be finite nonnegative seconds.');
    }
    const physicsSteps = this.physics.step(elapsedSeconds);
    this.entities.update(elapsedSeconds);
    return Object.freeze({elapsedSeconds, physicsSteps});
  }

  /** Release entities before their borrowed physics state, and make repeated destruction harmless. */
  destroy(): void {
    if(this.destroyed) {
      return;
    }
    this.destroyed = true;
    const errors = [];
    try {
      this.entities.destroy();
    } catch(error) {
      errors.push(error);
    }
    try {
      if(this.ownsPhysics) {
        this.physics.destroy();
      }
    } catch(error) {
      errors.push(error);
    }
    if(errors.length > 0) {
      throw new AggregateError(errors, 'World cleanup failed.');
    }
  }
}
