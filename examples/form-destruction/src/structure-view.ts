// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {createChunkMeshData} from 'joy-engine';
import type {ChunkStructure, MeshAssetData} from 'joy-engine';
import {DESTRUCTION_CONFIG, INTEGRITY_CONFIG, STRUCTURE_VIEW, SUPPORT_CONFIG} from './config/destruction-config.ts';
import type {StructureView} from './config/destruction-config.ts';

/**
 * Build structure presentation from committed engine state; never changes health or collision dimensions.
 * Debug views inset faces slightly to reveal boundaries, then color resistance or committed support paths.
 */
export function createStructureMeshData(structure: ChunkStructure, view: StructureView): MeshAssetData {
  const chunks = structure.activeChunks();
  if(view === STRUCTURE_VIEW.material) {
    return createChunkMeshData(chunks);
  }
  const health = new Map(structure.damageState().map(chunk => [chunk.chunkId, chunk.remainingHitPoints]));
  const support = new Map(structure.supportState().map(chunk => [chunk.chunkId, chunk]));
  const positions: number[] = [];
  const indices: number[] = [];
  const colors: number[] = [];

  for(const chunk of chunks) {
    const geometry = createChunkMeshData([{
      ...chunk,
      size: [chunk.size[0]*INTEGRITY_CONFIG.meshInset, chunk.size[1]*INTEGRITY_CONFIG.meshInset, chunk.size[2]*INTEGRITY_CONFIG.meshInset]
    }]);
    const vertexOffset = positions.length/3;
    const supportState = support.get(chunk.id);
    let color: readonly number[] = integrityColor(health.get(chunk.id) ?? chunk.hitPoints);
    if(view === STRUCTURE_VIEW.support && supportState) {
      color = supportState.anchored ? SUPPORT_CONFIG.anchoredColor : SUPPORT_CONFIG.verticalColor;
      if((supportState.lateralDistance ?? 0) > 0) {
        color = SUPPORT_CONFIG.bracedColor;
      }
    }
    positions.push(...geometry.positions);
    for(const index of geometry.indices) {
      indices.push(vertexOffset + index);
    }
    for(let vertex = 0; vertex < geometry.positions.length/3; vertex++) {
      colors.push(...color);
    }
  }
  return {positions, indices, colors};
}

/**
 * The shared absolute scale lets stronger frame chunks remain visibly distinct from ordinary masonry.
 */
function integrityColor(remaining: number) {
  const middle = DESTRUCTION_CONFIG.hitPoints/2;
  let first: readonly number[];
  let second: readonly number[];
  let fraction: number;
  if(remaining > DESTRUCTION_CONFIG.hitPoints) {
    first = INTEGRITY_CONFIG.intactColor;
    second = INTEGRITY_CONFIG.reinforcedColor;
    fraction = (remaining - DESTRUCTION_CONFIG.hitPoints)/(DESTRUCTION_CONFIG.frameHitPoints - DESTRUCTION_CONFIG.hitPoints);
  } else if(remaining > middle) {
    first = INTEGRITY_CONFIG.weakenedColor;
    second = INTEGRITY_CONFIG.intactColor;
    fraction = (remaining - middle)/middle;
  } else {
    first = INTEGRITY_CONFIG.criticalColor;
    second = INTEGRITY_CONFIG.weakenedColor;
    fraction = remaining/middle;
  }
  const blend = Math.max(0, Math.min(1, fraction));
  return first.map((component, axis) => component + (second[axis] - component)*blend);
}
