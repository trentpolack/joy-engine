// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {Vector3} from '../core/types.ts';

export type PhysicsResidency = 'cpu' | 'gpu';
export type PhysicsCollisionShape = Readonly<{id:string; center:Vector3; halfExtents:Vector3}>;
export type PhysicsMutationBatch = Readonly<{
  sequence:number;
  removedShapeIds:readonly string[];
  addedShapes:readonly PhysicsCollisionShape[];
  invalidatesContacts:boolean;
  rebuildsAccelerationStructure:boolean;
}>;

/**
 * Solver-neutral synchronization boundary for resident bodies, transforms, velocities, and shapes.
 * Gameplay-visible mutation batches complete before query(), avoiding asynchronous readback on aim
 * and character collision paths. GPU backends may retain simulation state on-device and publish only
 * compact event/readback records; CPU implementations remain the required fallback.
 */
export interface PhysicsBackend {
  readonly residency: PhysicsResidency;
  readonly activeBodyCount: number;
  applyMutations(batch: PhysicsMutationBatch): void | Promise<void>;
  step(elapsedSeconds:number): void | Promise<void>;
  query(origin:readonly number[], direction:readonly number[], maximumDistance:number):
    null | {shapeId:string; distance:number; point:Vector3};
  destroy():void;
}

/** Create a contact-invalidating batch for chunk removals at one simulation boundary. */
export function createRemovalMutationBatch(sequence:number, removedShapeIds:readonly string[]):PhysicsMutationBatch {
  if(!Number.isInteger(sequence) || sequence < 0 || removedShapeIds.some(id => !id)) {
    throw new RangeError('Physics mutation batches need a nonnegative sequence and nonempty shape ids.');
  }
  return Object.freeze({sequence, removedShapeIds:[...removedShapeIds], addedShapes:[],
    invalidatesContacts:removedShapeIds.length > 0, rebuildsAccelerationStructure:removedShapeIds.length > 0});
}
