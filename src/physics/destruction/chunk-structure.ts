// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {Vector3} from '../../core/types.ts';
import {ChunkSupportGraph} from './chunk-support.ts';
import type {ChunkSupportOptions, ChunkSupportState} from './chunk-support.ts';

export interface DestructibleChunkDefinition {
  id: string;
  center: Vector3;
  size: Vector3;
  material: string;
  hitPoints: number;
}

export interface RadialDamage {
  center: readonly number[];
  radius: number;
  damage: number;
}

export interface ChunkChangeSet {
  revision: number;
  /** All chunks leaving static geometry and queries at this boundary, including support detachments. */
  removed: readonly DestructibleChunkDefinition[];
  /** Subset of removed chunks released by support loss, retaining their remaining health. */
  detached: readonly DestructibleChunkDefinition[];
  activeChunkCount: number;
}

/**
 * Copied committed damage state; hit points use the same units as queued damage.
 */
export interface ChunkDamageState {
  chunkId: string;
  remainingHitPoints: number;
  maximumHitPoints: number;
  active: boolean;
}

interface ChunkState {
  definition: Readonly<DestructibleChunkDefinition>;
  hitPoints: number;
  active: boolean;
}

/**
 * Owns damage for stable, axis-aligned chunks and commits queued changes only at an explicit
 * simulation boundary. Coordinates and dimensions use caller-defined world units.
 */
export class ChunkStructure {
  private declare chunks: Map<string, ChunkState>;
  private declare queuedDamage: RadialDamage[];
  private declare revision: number;
  private support: ChunkSupportGraph | null;
  private supportSnapshot: ChunkSupportState[] = [];

  /**
   * Copy all definitions so generation data can be released or reused by the caller.
   * Omit support to retain damage-only removal; support-enabled consumers release unsupported chunks at commit().
   */
  constructor(definitions: readonly DestructibleChunkDefinition[], support?: ChunkSupportOptions) {
    this.chunks = new Map();
    this.queuedDamage = [];
    this.revision = 0;

    for(const definition of definitions) {
      validateDefinition(definition);
      if(this.chunks.has(definition.id)) {
        throw new Error(`Duplicate destructible chunk id: ${definition.id}`);
      }
      const copy = Object.freeze({
        id: definition.id,
        center: [...definition.center] as Vector3,
        size: [...definition.size] as Vector3,
        material: definition.material,
        hitPoints: definition.hitPoints
      });
      this.chunks.set(copy.id, {definition:copy, hitPoints:copy.hitPoints, active:true});
    }
    this.support = support ? new ChunkSupportGraph(this.activeChunks(), support) : null;
    this.refreshSupport();
  }

  /** Queue radial damage. Geometry and collision remain unchanged until commit(). */
  queueDamage(damage: RadialDamage) {
    if(damage.center.length !== 3 || !damage.center.every(Number.isFinite)) {
      throw new RangeError('Damage center needs three finite coordinates.');
    }
    if(!Number.isFinite(damage.radius) || damage.radius <= 0 || !Number.isFinite(damage.damage) || damage.damage <= 0) {
      throw new RangeError('Damage radius and amount must be finite positive values.');
    }
    this.queuedDamage.push({center:[...damage.center], radius:damage.radius, damage:damage.damage});
  }

  /**
   * Apply every queued hit atomically and return the chunks removed by this boundary.
   * Overlapping hits accumulate before removal, while already removed chunks are ignored.
   * With support enabled, evaluate the surviving contacts and detach unsupported chunks in the same revision.
   */
  commit(): ChunkChangeSet {
    const removed:DestructibleChunkDefinition[] = [];
    for(const damage of this.queuedDamage) {
      for(const chunk of this.chunks.values()) {
        if(!chunk.active || !sphereIntersectsBox(damage.center, damage.radius, chunk.definition)) {
          continue;
        }
        chunk.hitPoints-= damage.damage;
      }
    }
    this.queuedDamage = [];

    for(const chunk of this.chunks.values()) {
      if(chunk.active && chunk.hitPoints <= 0) {
        chunk.active = false;
        removed.push(chunk.definition);
      }
    }
    this.refreshSupport();
    const detached: DestructibleChunkDefinition[] = [];
    for(const state of this.supportSnapshot) {
      const chunk = this.chunks.get(state.chunkId)!;
      if(chunk.active && !state.supported) {
        chunk.active = false;
        detached.push(chunk.definition);
        removed.push(chunk.definition);
      }
    }
    if(removed.length) {
      this.revision+= 1;
    }
    return {revision:this.revision, removed, detached, activeChunkCount:this.activeChunks().length};
  }

  /** Restore original health and activity, discarding uncommitted hits. */
  reset(): ChunkChangeSet {
    this.queuedDamage = [];
    const restored:DestructibleChunkDefinition[] = [];
    for(const chunk of this.chunks.values()) {
      if(!chunk.active) {
        restored.push(chunk.definition);
      }
      chunk.active = true;
      chunk.hitPoints = chunk.definition.hitPoints;
    }
    this.revision+= 1;
    this.refreshSupport();
    return {revision:this.revision, removed:[], detached:[], activeChunkCount:this.chunks.size};
  }

  /** Active definitions are borrowed immutable records in stable generation order. */
  activeChunks() {
    return [...this.chunks.values()].filter(chunk => chunk.active).map(chunk => chunk.definition);
  }

  /**
   * Copy damage state in generation order. Impact-destroyed chunks have zero health;
   * support-detached chunks retain health while becoming inactive.
   * Queued damage is invisible until commit(); callers own the returned array and records.
   */
  damageState(): ChunkDamageState[] {
    return [...this.chunks.values()].map(chunk => ({
      chunkId: chunk.definition.id,
      remainingHitPoints: Math.max(0, chunk.hitPoints),
      maximumHitPoints: chunk.definition.hitPoints,
      active: chunk.active
    }));
  }

  /**
   * Copy committed static support diagnostics, or an empty array when support is disabled.
   */
  supportState(): ChunkSupportState[] {
    return this.supportSnapshot.map(chunk => ({...chunk}));
  }

  /** Nearest active chunk hit by a world-space ray, or null. */
  raycast(origin: readonly number[], direction: readonly number[], maximumDistance: number) {
    if(origin.length !== 3 || direction.length !== 3 || !Number.isFinite(maximumDistance) || maximumDistance <= 0) {
      throw new RangeError('Raycast needs three-component vectors and a positive maximum distance.');
    }
    let nearest:{chunk:Readonly<DestructibleChunkDefinition>;distance:number;point:Vector3}|null = null;
    for(const chunk of this.activeChunks()) {
      const distance = rayBoxDistance(origin, direction, chunk);
      if(distance === null || distance > maximumDistance || (nearest && distance >= nearest.distance)) {
        continue;
      }
      nearest = {chunk, distance, point:origin.map((value, axis) => value + direction[axis]*distance) as Vector3};
    }
    return nearest;
  }

  /** True when a player-sized axis-aligned probe can occupy the supplied world position. */
  canOccupy(center: readonly number[], halfExtents: readonly number[]) {
    if(center.length !== 3 || halfExtents.length !== 3 || !center.every(Number.isFinite)
        || !halfExtents.every(value => Number.isFinite(value) && value > 0)) {
      throw new RangeError('Occupancy queries need a finite center and positive three-component half extents.');
    }
    for(const chunk of this.activeChunks()) {
      if(boxesOverlap(center, halfExtents, chunk.center, chunk.size.map(value => value/2))) {
        return false;
      }
    }
    return true;
  }

  private refreshSupport() {
    this.supportSnapshot = this.support?.evaluate(new Set(this.activeChunks().map(chunk => chunk.id))) ?? [];
  }
}

function validateDefinition(definition: DestructibleChunkDefinition) {
  if(!definition.id || !definition.material || definition.center.length !== 3 || definition.size.length !== 3) {
    throw new TypeError('Chunks need an id, material, center, and size.');
  }
  if(!definition.center.every(Number.isFinite) || !definition.size.every(value => Number.isFinite(value) && value > 0)
      || !Number.isFinite(definition.hitPoints) || definition.hitPoints <= 0) {
    throw new RangeError('Chunk coordinates must be finite; dimensions and hit points must be positive.');
  }
}

function sphereIntersectsBox(center: readonly number[], radius: number, chunk: Readonly<DestructibleChunkDefinition>) {
  let distanceSquared = 0;
  for(let axis = 0; axis < 3; axis++) {
    const half = chunk.size[axis]/2;
    const closest = Math.max(chunk.center[axis] - half, Math.min(center[axis], chunk.center[axis] + half));
    distanceSquared+= (center[axis] - closest)**2;
  }
  return distanceSquared <= radius**2;
}

function rayBoxDistance(origin: readonly number[], direction: readonly number[], chunk: Readonly<DestructibleChunkDefinition>) {
  let minimum = 0;
  let maximum = Infinity;
  for(let axis = 0; axis < 3; axis++) {
    const half = chunk.size[axis]/2;
    const lower = chunk.center[axis] - half;
    const upper = chunk.center[axis] + half;
    if(Math.abs(direction[axis]) < 1e-9) {
      if(origin[axis] < lower || origin[axis] > upper) {
        return null;
      }
      continue;
    }
    const first = (lower - origin[axis])/direction[axis];
    const second = (upper - origin[axis])/direction[axis];
    minimum = Math.max(minimum, Math.min(first, second));
    maximum = Math.min(maximum, Math.max(first, second));
    if(maximum < minimum) {
      return null;
    }
  }
  return minimum;
}

function boxesOverlap(firstCenter: readonly number[], firstHalf: readonly number[], secondCenter: readonly number[], secondHalf: readonly number[]) {
  return [0, 1, 2].every(axis => Math.abs(firstCenter[axis] - secondCenter[axis]) < firstHalf[axis] + secondHalf[axis]);
}
