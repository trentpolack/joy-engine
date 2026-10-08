// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { PhysicsWorld } from './physics-world.ts';
import type { PhysicsBody } from './physics-body.ts';

import {RaycastVehicle, Vec3} from 'cannon-es';
import {eulerRotation, finiteNumber, vector} from './physics-values.ts';

export interface PhysicsWheelOptions {
  position: readonly number[];
  radius?: number;
  steering?: boolean;
  driven?: boolean;
  brake?: boolean;
  suspensionRestLength?: number;
  suspensionStiffness?: number;
  dampingCompression?: number;
  dampingRelaxation?: number;
  maxSuspensionTravel?: number;
  maxSuspensionForce?: number;
  frictionSlip?: number;
  rollInfluence?: number;
}
export interface PhysicsVehicleOptions {
  wheels: readonly PhysicsWheelOptions[];
  engineForce?: number;
  maxSteer?: number;
  brakeForce?: number;
  frictionSlip?: number;
}
export interface PhysicsVehicleInput {
  throttle?: number;
  steer?: number;
  brake?: number;
}

/**
 * Raycast suspension for a data-defined wheel array; tire contacts are rays, not rolling rigid bodies.
 * Construct through PhysicsWorld.createVehicle. World owns this adapter; chassis is borrowed and survives adapter destruction.
 */
export class PhysicsVehicle {
  declare engineForce: number;
  declare maxSteer: number;
  declare brakeForce: number;
  declare wheels: { position: Vec3; radius: number; steering: boolean; driven: boolean; brake: boolean; suspensionRestLength: number; suspensionStiffness: number; dampingCompression: number; dampingRelaxation: number; maxSuspensionTravel: number; maxSuspensionForce: number; frictionSlip: number; rollInfluence: number; }[];
  declare world: PhysicsWorld;
  declare chassis: PhysicsBody;
  declare disposed: boolean;
  declare backend: RaycastVehicle;

  /**
   * @internal Factory-only constructor; direct calls throw before attaching solver callbacks.
   * @param world
   * @param chassis
   * @param config
   * @param [capability]
   */
  constructor(world: PhysicsWorld, chassis: PhysicsBody, config: PhysicsVehicleOptions, capability?: symbol) {
    world.assertFactoryCapability(capability, 'createVehicle');
    this.engineForce = finiteNumber(config.engineForce ?? 400, 'engineForce', 0);
    this.maxSteer = finiteNumber(config.maxSteer ?? 0.45, 'maxSteer', 0, Math.PI/2);
    this.brakeForce = finiteNumber(config.brakeForce ?? 20, 'brakeForce', 0);
    const frictionSlip = finiteNumber(config.frictionSlip ?? 5, 'frictionSlip', 0);
    if(!Array.isArray(config.wheels) || config.wheels.length === 0) {
      throw new TypeError('A vehicle requires at least one wheel.');
    }
    // Validate every wheel before attaching any callback or mutating world ownership.
    this.wheels = config.wheels.map((wheel) => validateWheel(wheel, frictionSlip));
    this.world = world;
    this.chassis = chassis;
    this.disposed = false;
    /** @internal */
    this.backend = new RaycastVehicle({
      chassisBody: chassis.backend,
      indexRightAxis: 0,
      indexUpAxis: 1,
      indexForwardAxis: 2
    });
    for(const wheel of this.wheels) {
      this.backend.addWheel({
        chassisConnectionPointLocal: wheel.position,
        radius: wheel.radius,
        directionLocal: new Vec3(0, -1, 0),
        axleLocal: new Vec3(-1, 0, 0),
        suspensionRestLength: wheel.suspensionRestLength,
        suspensionStiffness: wheel.suspensionStiffness,
        dampingCompression: wheel.dampingCompression,
        dampingRelaxation: wheel.dampingRelaxation,
        maxSuspensionTravel: wheel.maxSuspensionTravel,
        maxSuspensionForce: wheel.maxSuspensionForce,
        frictionSlip: wheel.frictionSlip,
        rollInfluence: wheel.rollInfluence
      });
    }
    this.backend.addToWorld(world.backend);
  }

  /**
   * Replace intent; omitted fields reset to zero. Throttle/steer clamp to [-1,1], brake to [0,1].
   * Positive throttle drives local +Z; positive steer turns toward local +X.
   * @param input
   */
  setInput(input: PhysicsVehicleInput) {
    this.assertActive();
    const throttle = Math.max(-1, Math.min(1, finiteNumber(input.throttle ?? 0, 'throttle')));
    const steer = Math.max(-1, Math.min(1, finiteNumber(input.steer ?? 0, 'steer')));
    const brake = Math.max(0, Math.min(1, finiteNumber(input.brake ?? 0, 'brake')));
    for(let index = 0; index < this.wheels.length; index+= 1) {
      const wheel = this.wheels[index];
      // Cannon's tire forward tangent is -Z with this Y-up axis convention.
      this.backend.applyEngineForce(wheel.driven ? -throttle*this.engineForce : 0, index);
      this.backend.setSteeringValue(wheel.steering ? steer*this.maxSteer : 0, index);
      this.backend.setBrake(wheel.brake ? brake*this.brakeForce : 0, index);
    }
    this.chassis.backend.wakeUp();
  }

  /** Copied wheel transforms in world meters and XYZ Euler radians; radius remains configuration data. */
  getWheelTransforms() {
    this.assertActive();
    return this.wheels.map((wheel, index) => {
      this.backend.updateWheelTransform(index);
      const transform = this.backend.getWheelTransformWorld(index);
      return {position: transform.position.toArray(), rotation: eulerRotation(transform.quaternion)};
    });
  }

  /** Unsigned chassis speed in meters/second. */
  getSpeed() {
    this.assertActive();
    return this.chassis.backend.velocity.length();
  }

  /** Tire contacts recorded during the most recent fixed step. */
  getGroundedWheelCount() {
    this.assertActive();
    return this.backend.numWheelsOnGround;
  }

  /** Remove the solver's pre-step callback while retaining the borrowed chassis. Idempotent. */
  destroy() {
    if(this.disposed) {
      return;
    }

    this.setInput({});
    this.backend.removeFromWorld(this.world.backend);

    // Cannon's convenience detach also removes the chassis, which belongs to our world.
    this.world.backend.addBody(this.chassis.backend);
    this.world.vehicles.delete(this);
    if(this.chassis.vehicle === this) {
      this.chassis.vehicle = null;
    }
    this.disposed = true;
  }

  /** Reject solver access after any required owner has been disposed. @private */
  private assertActive() {
    if(this.disposed || this.world.disposed || this.chassis.disposed) {
      throw new Error('Physics vehicle has been destroyed.');
    }
  }
}

/**
 * Copy and validate a wheel before the vehicle acquires solver callbacks.
 * @param wheel Borrowed chassis-local geometry and tuning.
 * @param frictionSlip Vehicle-wide default tire grip.
 */
function validateWheel(wheel: PhysicsWheelOptions, frictionSlip: number) {
  for(const flag of ['steering', 'driven', 'brake']) {
    const value = wheel[ ((flag) as 'steering' | 'driven' | 'brake')];
    if(value !== undefined && typeof value !== 'boolean') {
      throw new TypeError(`Wheel ${flag} must be boolean.`);
    }
  }

  return {
    position: vector(wheel.position, 'wheel.position'),
    radius: finiteNumber(wheel.radius ?? 0.35, 'wheel.radius', Number.EPSILON),
    steering: wheel.steering ?? false,
    driven: wheel.driven ?? false,
    brake: wheel.brake ?? false,
    suspensionRestLength: finiteNumber(wheel.suspensionRestLength ?? 0.3, 'wheel.suspensionRestLength', 0),
    suspensionStiffness: finiteNumber(wheel.suspensionStiffness ?? 30, 'wheel.suspensionStiffness', 0),
    dampingCompression: finiteNumber(wheel.dampingCompression ?? 4.4, 'wheel.dampingCompression', 0),
    dampingRelaxation: finiteNumber(wheel.dampingRelaxation ?? 2.3, 'wheel.dampingRelaxation', 0),
    maxSuspensionTravel: finiteNumber(wheel.maxSuspensionTravel ?? 0.3, 'wheel.maxSuspensionTravel', 0),
    maxSuspensionForce: finiteNumber(wheel.maxSuspensionForce ?? 100000, 'wheel.maxSuspensionForce', 0),
    frictionSlip: finiteNumber(wheel.frictionSlip ?? frictionSlip, 'wheel.frictionSlip', 0),
    rollInfluence: finiteNumber(wheel.rollInfluence ?? 0.01, 'wheel.rollInfluence', 0, 1)
  };
}
