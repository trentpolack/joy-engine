// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {DestructibleChunkDefinition} from './chunk-structure.ts';

/**
 * Optional Y-up masonry support approximation, in the same world units as chunk geometry.
 * Face contacts carry support upward and sideways; horizontal offsets spend the cantilever allowance.
 * This is a geometric support rule, not a stress, torque, or material-load solver.
 */
export interface ChunkSupportOptions {
  groundHeight: number;
  maximumCantileverDistance: number;
  contactTolerance?: number;
}

/**
 * Copied static support diagnostics. Null distance means no permitted route to a ground anchor.
 * Released rigid bodies are outside this graph, even after they land on the ground.
 */
export interface ChunkSupportState {
  chunkId: string;
  anchored: boolean;
  supported: boolean;
  lateralDistance: number | null;
}

type SupportEdge = {target: string; distance: number};
const DEFAULT_CONTACT_TOLERANCE = 0.0001;
const HORIZONTAL_AXES = [0, 2] as const;

/**
 * Owns immutable contact topology for a fixed collection of axis-aligned chunks.
 * Damage only changes active IDs; callers own every returned diagnostic record.
 */
export class ChunkSupportGraph {
  private edges = new Map<string, SupportEdge[]>();
  private anchors = new Set<string>();
  private maximumDistance: number;

  constructor(chunks: readonly Readonly<DestructibleChunkDefinition>[], options: ChunkSupportOptions) {
    const tolerance = options.contactTolerance ?? DEFAULT_CONTACT_TOLERANCE;
    if(!Number.isFinite(options.groundHeight) || !Number.isFinite(options.maximumCantileverDistance)
        || options.maximumCantileverDistance < 0 || !Number.isFinite(tolerance) || tolerance < 0) {
      throw new RangeError('Chunk support requires a finite ground height and nonnegative span and contact tolerance.');
    }
    this.maximumDistance = options.maximumCantileverDistance;
    for(const chunk of chunks) {
      this.edges.set(chunk.id, []);
      const bottom = chunk.center[1] - chunk.size[1]/2;
      const top = chunk.center[1] + chunk.size[1]/2;
      if(bottom <= options.groundHeight + tolerance && top > options.groundHeight) {
        this.anchors.add(chunk.id);
      }
    }
    for(const source of chunks) {
      for(const target of chunks) {
        if(source.id === target.id) {
          continue;
        }
        const horizontalDistance = Math.hypot(target.center[0] - source.center[0], target.center[2] - source.center[2]);
        const verticalGap = (target.center[1] - target.size[1]/2) - (source.center[1] + source.size[1]/2);
        if(Math.abs(verticalGap) <= tolerance && HORIZONTAL_AXES.every(axis => overlaps(source, target, axis, tolerance))) {
          // Only aligned stacks are free; staggering upward cannot bypass the sideways limit.
          this.edges.get(source.id)!.push({target: target.id, distance: horizontalDistance});
          continue;
        }
        for(const axis of HORIZONTAL_AXES) {
          const otherAxis = 2 - axis;
          const faceGap = Math.abs(target.center[axis] - source.center[axis]) - (source.size[axis] + target.size[axis])/2;
          if(Math.abs(faceGap) <= tolerance && overlaps(source, target, 1, tolerance)
              && overlaps(source, target, otherAxis, tolerance)) {
            this.edges.get(source.id)!.push({target: target.id, distance: horizontalDistance});
            break;
          }
        }
      }
    }
  }

  /**
   * Resolve the shortest permitted sideways travel from active ground anchors.
   * Nonnegative edge costs and a visited set bound the search to one visit per active chunk.
   */
  evaluate(activeIds: ReadonlySet<string>): ChunkSupportState[] {
    const distances = new Map<string, number>();
    const visited = new Set<string>();
    for(const id of activeIds) {
      distances.set(id, this.anchors.has(id) ? 0 : Infinity);
    }
    while(visited.size < activeIds.size) {
      let nearest: string | null = null;
      let nearestDistance = Infinity;
      for(const [id, distance] of distances) {
        if(!visited.has(id) && distance < nearestDistance) {
          nearest = id;
          nearestDistance = distance;
        }
      }
      if(nearest === null || nearestDistance > this.maximumDistance) {
        break;
      }
      visited.add(nearest);
      for(const edge of this.edges.get(nearest)!) {
        const candidate = nearestDistance + edge.distance;
        if(activeIds.has(edge.target) && candidate < distances.get(edge.target)!) {
          distances.set(edge.target, candidate);
        }
      }
    }
    return [...this.edges.keys()].map(chunkId => ({
      chunkId,
      anchored: activeIds.has(chunkId) && this.anchors.has(chunkId),
      supported: visited.has(chunkId),
      lateralDistance: visited.has(chunkId) ? distances.get(chunkId)! : null
    }));
  }
}

function overlaps(first: Readonly<DestructibleChunkDefinition>, second: Readonly<DestructibleChunkDefinition>, axis: number, tolerance: number) {
  return Math.abs(first.center[axis] - second.center[axis]) < (first.size[axis] + second.size[axis])/2 - tolerance;
}
