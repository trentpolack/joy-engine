// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { PRIMITIVE_SHAPE } from '../rendering/geometry/constants.ts';
import type { PhysicsWorld } from './physics-world.ts';
import type { PhysicsVehicle } from './physics-vehicle.ts';
import {Body, Box, Material, Plane, Quaternion, Sphere, Vec3} from 'cannon-es';
import {eulerRotation, finiteNumber, vector} from './physics-values.ts';

export interface PhysicsBodyOptions {
  shape: typeof PRIMITIVE_SHAPE.BOX | typeof PRIMITIVE_SHAPE.SPHERE | typeof PRIMITIVE_SHAPE.PLANE;
  mass?: number;
  halfExtents?: readonly number[];
  radius?: number;
  position?: readonly number[];
  rotation?: readonly number[];
  friction?: number;
  restitution?: number;
  linearDamping?: number;
  angularDamping?: number;
}
export interface PhysicsTransform {
  position?: readonly number[];
  rotation?: readonly number[];
}

/**
 * World-owned rigid body. Construction requires PhysicsWorld.createBody; mass 0 is static.
 * World coordinates are meters, Y up, +Z forward; XYZ Euler radians apply X, then Y, then Z.
 */
export class PhysicsBody {
  declare world: PhysicsWorld;
  declare disposed: boolean;
  declare vehicle: PhysicsVehicle | null;
  declare backend: Body;

  /**
   * @internal Factory-only constructor; direct calls throw before allocating a solver body.
   * @param world
   * @param options
   * @param [capability]
   */
  constructor(world: PhysicsWorld, options: PhysicsBodyOptions, capability?: symbol) {
    world.assertFactoryCapability(capability, 'createBody');
    const mass = finiteNumber(options.mass ?? 0, 'mass', 0);
    const friction = finiteNumber(options.friction ?? 0.5, 'friction', 0);
    const restitution = finiteNumber(options.restitution ?? 0, 'restitution', 0, 1);
    const position = vector(options.position ?? [0, 0, 0], 'position');
    const rotation = vector(options.rotation ?? [0, 0, 0], 'rotation');
    const linearDamping = finiteNumber(options.linearDamping ?? 0.01, 'linearDamping', 0, 1);
    const angularDamping = finiteNumber(options.angularDamping ?? 0.1, 'angularDamping', 0, 1);
    const shape = createShape(options);
    if(options.shape === PRIMITIVE_SHAPE.PLANE && mass !== 0) {
      throw new RangeError('Infinite planes must be static (mass 0).');
    }

    // The world owns this body; a vehicle borrows it and is detached first.
    this.world = world;
    this.disposed = false;

    this.vehicle = null;
    /** @internal Solver state stays behind the engine adapter. */
    this.backend = new Body({
      mass,
      position,
      material: new Material({friction, restitution}),
      linearDamping,
      angularDamping
    });
    // Cannon's intrinsic ZYX order matches the renderer's world-axis X-then-Y-then-Z order.
    this.backend.quaternion.setFromEuler(rotation.x, rotation.y, rotation.z, 'ZYX');
    const shapeRotation = new Quaternion();
    if(options.shape === PRIMITIVE_SHAPE.PLANE) {
      // Solver planes point +Z; authored planes point +Y before entity rotation.
      shapeRotation.setFromEuler(-Math.PI/2, 0, 0, 'ZYX');
    }
    this.backend.addShape(shape, new Vec3(), shapeRotation);
  }

  /**
   * Copied transform; mutating the result never changes the simulation.
   */
  getTransform(): {
    position: [
        number,
        number,
        number
    ];
    rotation: [
        number,
        number,
        number
    ];
} {
    this.assertActive();
    const {x, y, z} = this.backend.position;
    return {position: [x, y, z], rotation: eulerRotation(this.backend.quaternion)};
  }

  /**
   * Teleport in world space without changing velocity.
   * @param transform
   */
  setTransform(transform: PhysicsTransform) {
    this.assertActive();
    const position = transform.position ? vector(transform.position, 'position') : null;
    const rotation = transform.rotation ? vector(transform.rotation, 'rotation') : null;
    if(position) {
      this.backend.position.copy(position);
      this.backend.previousPosition.copy(position);
      this.backend.interpolatedPosition.copy(position);
    }
    if(rotation) {
      this.backend.quaternion.setFromEuler(rotation.x, rotation.y, rotation.z, 'ZYX');
      this.backend.previousQuaternion.copy(this.backend.quaternion);
      this.backend.interpolatedQuaternion.copy(this.backend.quaternion);
    }
    this.backend.aabbNeedsUpdate = true;
    this.backend.wakeUp();
  }

  /** Copied world-space velocity in meters/second. */
  getVelocity() {
    this.assertActive();
    return this.backend.velocity.toArray();
  }

  /**
   * Set world-space velocity in meters/second.
   * @param velocity
   */
  setVelocity(velocity: readonly number[]) {
    this.assertActive();
    this.backend.velocity.copy(vector(velocity, 'velocity'));
    this.backend.wakeUp();
  }

  /**
   * Force in newtons until the next fixed step; optional application point is world space.
   * @param force
   * @param [point]
   */
  applyForce(force: readonly number[], point?: readonly number[]) {
    this.assertActive();
    this.backend.applyForce(vector(force, 'force'), this.relativePoint(point));
  }

  /**
   * Instantaneous impulse in newton-seconds; optional application point is world space.
   * @param impulse
   * @param [point]
   */
  applyImpulse(impulse: readonly number[], point?: readonly number[]) {
    this.assertActive();
    this.backend.applyImpulse(vector(impulse, 'impulse'), this.relativePoint(point));
  }

  /** Detach dependent vehicle, remove this body, and release ownership. Idempotent. */
  destroy() {
    if(this.disposed) {
      return;
    }
    this.vehicle?.destroy();
    this.world.backend.removeBody(this.backend);
    this.world.bodies.delete(this);
    this.disposed = true;
  }

  /** Reject solver access after either the body or its owner is disposed. @internal */
  assertActive() {
    if(this.disposed || this.world.disposed) {
      throw new Error('Physics body has been destroyed.');
    }
  }

  /**
   * Convert a world-space application point to the solver offset from the center of mass.
   * @private
   * @param point Omission applies at the center of mass.
   */
  private relativePoint(point: readonly number[] | undefined) {
    if(!point) {
      return new Vec3();
    }
    return vector(point, 'point').vsub(this.backend.position);
  }
}

/**
 * Validate collider dimensions and allocate a shape before attaching it to a body.
 * @param options
 */
function createShape(options: PhysicsBodyOptions) {
  switch(options.shape) {
    case PRIMITIVE_SHAPE.BOX: {
      const halfExtents = vector(options.halfExtents ?? [0.5, 0.5, 0.5], 'halfExtents');
      for(const extent of halfExtents.toArray()) {
        if(extent <= 0) {
          throw new RangeError('Box halfExtents must be positive.');
        }
      }
      return new Box(halfExtents);
    }
    case PRIMITIVE_SHAPE.SPHERE: {
      const radius = finiteNumber(options.radius ?? 0.5, 'radius', Number.EPSILON);
      return new Sphere(radius);
    }
    case PRIMITIVE_SHAPE.PLANE:
      return new Plane();
    default:
      throw new TypeError(`Unsupported physics shape: ${options.shape}`);
  }
}
