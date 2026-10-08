// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { FLOATS_PER_VERTEX } from '../geometry/geometry.ts';

export interface SceneTemporalRange {
  identity: object;
  topology: object;
  firstVertex: number;
  vertexCount: number;
}

interface SceneTemporalMesh {
  topology: object;
  positions: Float32Array<ArrayBuffer>;
}

export interface SceneTemporalSnapshot {
  scene: object;
  viewProjection: Float32Array<ArrayBuffer>;
  meshes: ReadonlyMap<object, SceneTemporalMesh>;
}

export interface SceneTemporalInput {
  scene: object;
  vertices: readonly number[] | Float32Array;
  ranges: readonly SceneTemporalRange[];
  viewProjection: ArrayLike<number>;
  resetHistory?: boolean;
}

/**
 * Match the visible opaque queue against the last successfully submitted frame.
 * XYZ/RGBA vertices are borrowed; the returned snapshot owns copies of world positions
 * and the unjittered column-major camera matrix. Commit it only after successful rendering.
 * Identity is the MeshInstance; topology is its immutable MeshAsset. Queue order may change.
 * Unidentified transient geometry cannot safely reuse temporal history. Camera cuts, scene
 * changes and explicit resets also discard correspondence. Hidden meshes leave no stale history.
 */
export function prepareSceneTemporalFrame(previous: SceneTemporalSnapshot | null, input: SceneTemporalInput) {
  const {scene, vertices, ranges, viewProjection} = input;
  if(vertices.length % (FLOATS_PER_VERTEX*3)) {
    throw new RangeError('Temporal scene vertices must contain complete XYZ/RGBA triangles.');
  }
  if(viewProjection.length !== 16 || !Array.from(viewProjection).every(Number.isFinite)) {
    throw new RangeError('Temporal scene camera matrix must contain 16 finite values.');
  }
  const discontinuity = !previous || previous.scene !== scene || Boolean(input.resetHistory);
  const history = discontinuity ? null : previous;
  const vertexCount = vertices.length/FLOATS_PER_VERTEX;
  const previousPositions = new Float32Array(vertexCount*3);
  const covered = new Uint8Array(vertexCount);
  const meshes = new Map<object, SceneTemporalMesh>();
  let resetHistory = discontinuity;

  for(let vertex = 0; vertex < vertexCount; vertex++) {
    const offset = vertex*FLOATS_PER_VERTEX;
    previousPositions.set([vertices[offset], vertices[offset + 1], vertices[offset + 2]], vertex*3);
  }
  for(const range of ranges) {
    if(!Number.isInteger(range.firstVertex) || !Number.isInteger(range.vertexCount) ||
      range.firstVertex < 0 || range.vertexCount < 0 || range.firstVertex % 3 || range.vertexCount % 3 ||
      range.firstVertex + range.vertexCount > vertexCount || meshes.has(range.identity)) {
      throw new RangeError('Temporal scene ranges must identify unique complete triangle runs.');
    }
    for(let vertex = range.firstVertex; vertex < range.firstVertex + range.vertexCount; vertex++) {
      if(covered[vertex]) {
        throw new RangeError('Temporal scene ranges must not overlap.');
      }
      covered[vertex] = 1;
    }
    const firstPosition = range.firstVertex*3;
    const positions = previousPositions.slice(firstPosition, firstPosition + range.vertexCount*3);
    const priorMesh = history?.meshes.get(range.identity);
    if(priorMesh?.topology === range.topology && priorMesh.positions.length === positions.length) {
      previousPositions.set(priorMesh.positions, firstPosition);
    } else {
      resetHistory = true;
    }
    meshes.set(range.identity, {topology: range.topology, positions});
  }
  if(covered.some(value => value === 0) || (history && history.meshes.size !== meshes.size)) {
    resetHistory = true;
  }
  const matrix = new Float32Array(viewProjection);
  const snapshot: SceneTemporalSnapshot = {scene, viewProjection: matrix, meshes};
  return {
    previousPositions,
    previousViewProjection: history?.viewProjection ?? matrix,
    resetHistory,
    snapshot
  };
}
