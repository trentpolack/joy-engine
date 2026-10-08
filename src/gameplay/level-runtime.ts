// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { PRIMITIVE_SHAPE } from '../rendering/geometry/constants.ts';
import type { EntityRegistry } from './entity-registry.ts';
import type { PhysicsBodyOptions, PhysicsBody } from '../physics/physics-body.ts';
import type { PhysicsVehicleOptions, PhysicsVehicle } from '../physics/physics-vehicle.ts';
import type { LevelData } from './level-document.ts';
import { EntityWorld } from './entity-world.ts';
import { parseLevel } from './level-document.ts';
import { PhysicsWorld } from '../physics/physics-world.ts';

export interface LevelRuntimeOptions {
  registry: EntityRegistry;
  physics?: PhysicsWorld;
}

/**
 * Instantiates a copied level. Owns entities and their physics handles; a supplied physics world is borrowed.
 * `physics` components use local collider dimensions; entity scale is applied once at construction.
 * `vehicle` components use the PhysicsWorld vehicle contract, in chassis-local meters.
 * Physics handles are attached during construction; later EntityWorld spawns require their own setup.
 */
export class LevelRuntime {
  declare level: LevelData;
  declare world: EntityWorld;
  declare physics: PhysicsWorld;
  declare ownsPhysics: boolean;
  declare bodies: Map<string, PhysicsBody>;
  declare vehicles: Map<string, PhysicsVehicle>;
  declare destroyed: boolean;

  /**
   * @param level
   * @param options
   */
  constructor(level: LevelData, {registry, physics}: LevelRuntimeOptions) {
    this.level = parseLevel(JSON.stringify(level));
    if(this.level.entities.some(entity => entity.template)) {
      throw new Error('Resolve linked objects with resolveLevelObjects before creating LevelRuntime.');
    }
    this.world = new EntityWorld({registry});
    this.physics = physics ?? new PhysicsWorld();
    this.ownsPhysics = !physics;

    this.bodies = new Map();

    this.vehicles = new Map();
    this.destroyed = false;
    try {
      for(const definition of this.level.entities) {
        const entity = this.world.spawn(definition);
        const collider = entity.components.physics;
        const vehicleConfig = entity.components.vehicle;
        if(vehicleConfig !== undefined && collider === undefined) {
          throw new TypeError(`Entity ${entity.id}: vehicle requires a physics component.`);
        }
        if(collider === undefined) {
          continue;
        }
        const config = componentRecord(collider, 'physics');
        const scale = entity.transform.scale;
        if(scale.some(value => value <= 0)) {
          throw new RangeError('Physics entity scale must be positive.');
        }
        // The backend validates shape, mass and material values before allocating solver resources.
        if(config.shape !== PRIMITIVE_SHAPE.BOX && config.shape !== PRIMITIVE_SHAPE.SPHERE && config.shape !== PRIMITIVE_SHAPE.PLANE) {
          throw new TypeError('Physics shape must be box, sphere or plane.');
        }
        const options = {
          ...config,
          shape: config.shape,
          position: entity.transform.position,
          rotation: entity.transform.rotation
        };
        if(config.shape === PRIMITIVE_SHAPE.BOX) {
          const extents = config.halfExtents ?? [0.5, 0.5, 0.5];
          if(!Array.isArray(extents) || extents.length !== 3 || extents.some(value => typeof value !== 'number' || !Number.isFinite(value) || value <= 0)) {
            throw new TypeError('Box halfExtents require three finite positive numbers.');
          }
          Object.assign(options, {halfExtents: extents.map((value, index) => value*scale[index])});
        } else if(config.shape === PRIMITIVE_SHAPE.SPHERE) {
          if(scale[0] !== scale[1] || scale[1] !== scale[2]) {
            throw new RangeError('Sphere physics requires uniform entity scale.');
          }
          const radius = config.radius ?? 0.5;
          if(typeof radius !== 'number' || !Number.isFinite(radius) || radius <= 0) {
            throw new TypeError('Sphere radius must be finite and positive.');
          }
          Object.assign(options, {radius: radius*scale[0]});
        }
        const body = this.physics.createBody(((options) as PhysicsBodyOptions));
        entity.own(body);
        this.bodies.set(entity.id, body);
        if(vehicleConfig !== undefined) {
          const vehicleOptions = componentRecord(vehicleConfig, 'vehicle');
          if(scale[0] !== scale[1] || scale[1] !== scale[2]) {
            throw new RangeError('Vehicle physics requires uniform entity scale.');
          }
          const scaledOptions = scaleVehicle(vehicleOptions, scale[0]);
          const vehicle = this.physics.createVehicle(body,  ((scaledOptions) as PhysicsVehicleOptions));
          entity.own(vehicle);
          this.vehicles.set(entity.id, vehicle);
        }
      }
    } catch(error) {
      try {
        this.destroy();
      } catch(cleanupError) {
        throw new AggregateError([error, cleanupError], 'Level setup and rollback failed.');
      }
      throw error;
    }
  }

  /**
   * Update character responses, advance bounded physics in seconds, and copy world transforms to entities.
   * Controllers should be updated before this call. Authored definitions remain unchanged.
   * @param seconds
   */
  step(seconds: number) {
    if(this.destroyed) {
      throw new Error('LevelRuntime is destroyed.');
    }
    if(!Number.isFinite(seconds) || seconds < 0) {
      throw new RangeError('Runtime elapsed seconds must be finite and nonnegative.');
    }
    this.world.update(Math.min(seconds, 0.1));
    this.physics.step(seconds);
    for(const [id, body] of this.bodies) {
      const entity = this.world.get(id);
      if(!entity || body.disposed) {
        this.bodies.delete(id);
        this.vehicles.delete(id);
        continue;
      }
      const transform = body.getTransform();
      entity.transform.position = [...transform.position];
      entity.transform.rotation = [...transform.rotation];
    }
  }

  /**
   * Borrow a live body.
   * @param id
   */
  getBody(id: string) {
    const body = this.bodies.get(id);
    if(!this.world.get(id) || !body || body.disposed) {
      this.bodies.delete(id);
      this.vehicles.delete(id);
      return null;
    }
    return body;
  }

  /**
   * Borrow a live raycast vehicle.
   * @param id
   */
  getVehicle(id: string) {
    const vehicle = this.vehicles.get(id);
    if(!this.getBody(id) || !vehicle || vehicle.disposed) {
      this.vehicles.delete(id);
      return null;
    }
    return vehicle;
  }

  /**
   * Release entities before the owned solver; repeated disposal is harmless. */
  destroy() {
    if(this.destroyed) {
      return;
    }
    this.destroyed = true;
    try {
      this.world.destroy();
    } finally {
      this.vehicles.clear();
      this.bodies.clear();
      if(this.ownsPhysics) {
        this.physics.destroy();
      }
    }
  }
}

/**
 * Read a borrowed component record before interpreting its physics-specific fields.
 * @param value
 * @param name Component label for validation errors.
 */
function componentRecord(value: unknown, name: string): Record<string, unknown> {
  if(!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new TypeError(`${name} component must be an object.`);
  }
  return  ((value) as Record<string, unknown>);
}

/**
 * Scale tire geometry with the chassis. Solver force tuning stays in authored units.
 * @param config
 * @param scale
 */
function scaleVehicle(config: Record<string, unknown>, scale: number) {
  if(!Array.isArray(config.wheels)) {
    throw new TypeError('Vehicle wheels must be an array.');
  }
  return {
    ...config,
    wheels: config.wheels.map(value => {
      const wheel = componentRecord(value, 'wheel');
      if(!Array.isArray(wheel.position) || wheel.position.length !== 3 || wheel.position.some(item => typeof item !== 'number' || !Number.isFinite(item))) {
        throw new TypeError('Wheel position requires three finite numbers.');
      }
      const radius = wheel.radius ?? 0.35;
      const suspensionRestLength = wheel.suspensionRestLength ?? 0.3;
      const maxSuspensionTravel = wheel.maxSuspensionTravel ?? 0.3;
      for(const value of [radius, suspensionRestLength, maxSuspensionTravel]) {
        if(typeof value !== 'number' || !Number.isFinite(value) || value < 0) {
          throw new TypeError('Wheel dimensions must be finite and nonnegative.');
        }
      }
      return {
        ...wheel,
        position: wheel.position.map(value => Number(value)*scale),
        radius: Number(radius)*scale,
        suspensionRestLength: Number(suspensionRestLength)*scale,
        maxSuspensionTravel: Number(maxSuspensionTravel)*scale
      };
    })
  };
}
