// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { PhysicsBodyOptions } from './physics-body.ts';
import type { PhysicsVehicleOptions } from './physics-vehicle.ts';

import {RaycastResult, World} from 'cannon-es';
import {PhysicsBody} from './physics-body.ts';
import {PhysicsVehicle} from './physics-vehicle.ts';
import {finiteNumber, vector} from './physics-values.ts';

export interface PhysicsWorldOptions {
  gravity?: readonly number[];
  fixedStep?: number;
  maxSubSteps?: number;
}

/**
 * Owns a solver and every body/vehicle created through it. Independent instances share no gameplay state.
 * Uses discrete collisions: thin surfaces and very fast bodies can tunnel; raycast tires have no solid wheel collider.
 */
export class PhysicsWorld {
  declare fixedStep: number;
  declare maxSubSteps: number;
  declare accumulator: number;
  declare disposed: boolean;
  declare backend: World;
  declare bodies: Set<PhysicsBody>;
  declare vehicles: Set<PhysicsVehicle>;

  // Instance-owned capability never leaves the factory/constructor call boundary.
  #creationCapability = Symbol('physics factory');

  /**
   * Create an independent Y-up solver with bounded fixed-step work.
   * @param [options] Gravity is meters/second²; fixedStep is seconds.
   */
  constructor(options: PhysicsWorldOptions = {}) {
    // Fixed work limits prevent hidden-tab catch-up from stalling the frame.
    this.fixedStep = finiteNumber(options.fixedStep ?? 1/60, 'fixedStep', 1/1000, 1);
    this.maxSubSteps = finiteNumber(options.maxSubSteps ?? 5, 'maxSubSteps', 1, 120);
    if(!Number.isInteger(this.maxSubSteps)) {
      throw new RangeError('maxSubSteps must be an integer.');
    }
    this.accumulator = 0;
    this.disposed = false;
    /** @internal */
    this.backend = new World({gravity: vector(options.gravity ?? [0, -9.81, 0], 'gravity')});
    /** @internal */
    this.bodies = new Set();
    /** @internal */
    this.vehicles = new Set();
  }

  /**
   * Create a world-owned rigid body; invalid configuration leaves the world unchanged.
   * @param options
   */
  createBody(options: PhysicsBodyOptions) {
    this.assertActive();
    const body = new PhysicsBody(this, options, this.#creationCapability);
    this.backend.addBody(body.backend);
    this.bodies.add(body);
    return body;
  }

  /**
   * Attach one vehicle to an owned dynamic chassis. The vehicle borrows its chassis.
   * @param body
   * @param config
   */
  createVehicle(body: PhysicsBody, config: PhysicsVehicleOptions) {
    this.assertActive();
    if(!this.bodies.has(body) || body.disposed || body.backend.mass <= 0 || body.vehicle) {
      throw new Error('Vehicle chassis must be an unattached dynamic body owned by this world.');
    }
    const vehicle = new PhysicsVehicle(this, body, config, this.#creationCapability);
    this.vehicles.add(vehicle);
    body.vehicle = vehicle;
    return vehicle;
  }

  /**
   * Advance by elapsed seconds; returns executed fixed steps. Excess catch-up time is discarded.
   * @param elapsedSeconds
   */
  step(elapsedSeconds: number) {
    this.assertActive();
    finiteNumber(elapsedSeconds, 'elapsedSeconds', 0);
    this.accumulator = Math.min(this.accumulator + elapsedSeconds, this.fixedStep*this.maxSubSteps);
    let steps = 0;
    while(this.accumulator + 1e-12 >= this.fixedStep && steps < this.maxSubSteps) {
      this.backend.step(this.fixedStep);
      this.accumulator = Math.max(0, this.accumulator - this.fixedStep);
      steps+= 1;
    }
    return steps;
  }

  /**
   * Closest hit along a world-space segment. Result vectors are copied; body is borrowed.
   * @param from
   * @param to
   */
  raycast(from: readonly number[], to: readonly number[]) {
    this.assertActive();
    const result = new RaycastResult();
    const hasHit = this.backend.raycastClosest(vector(from, 'from'), vector(to, 'to'), {skipBackfaces: true}, result);
    if(!hasHit) {
      return null;
    }
    const body = [...this.bodies].find((candidate) => candidate.backend === result.body);
    if(!body) {
      return null;
    }
    return {
      body,
      point: result.hitPointWorld.toArray(),
      normal: result.hitNormalWorld.toArray(),
      distance: result.distance
    };
  }

  /** Release vehicle callbacks before bodies; repeated destruction is harmless. */
  destroy() {
    if(this.disposed) {
      return;
    }
    for(const vehicle of [...this.vehicles]) {
      vehicle.destroy();
    }
    for(const body of [...this.bodies]) {
      body.destroy();
    }
    this.accumulator = 0;
    this.disposed = true;
  }

  /**
   * @internal Reject direct adapter construction before resources can be allocated.
   * @param capability
   * @param factory
   */
  assertFactoryCapability(capability: symbol | undefined, factory: 'createBody' | 'createVehicle') {
    if(capability !== this.#creationCapability) {
      throw new Error(`Physics adapters must be constructed through PhysicsWorld.${factory}().`);
    }
    this.assertActive();
  }

  /** Reject creation and simulation work after world disposal. @private */
  private assertActive() {
    if(this.disposed) {
      throw new Error('Physics world has been destroyed.');
    }
  }
}
