// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {DestructibleChunkDefinition} from './chunk-structure.ts';
import type {MeshAssetData} from '../../core/types.ts';

/** Build closed box geometry. Every removed neighbor therefore reveals an authored interior face. */
export function createChunkMeshData(chunks: readonly Readonly<DestructibleChunkDefinition>[]):MeshAssetData {
  const positions:number[] = [];
  const indices:number[] = [];
  const colors:number[] = [];
  const faces = [
    {normal:[0,0,-1], corners:[[-1,-1,-1],[1,-1,-1],[1,1,-1],[-1,1,-1]]},
    {normal:[0,0,1], corners:[[-1,-1,1],[-1,1,1],[1,1,1],[1,-1,1]]},
    {normal:[-1,0,0], corners:[[-1,-1,-1],[-1,1,-1],[-1,1,1],[-1,-1,1]]},
    {normal:[1,0,0], corners:[[1,-1,-1],[1,-1,1],[1,1,1],[1,1,-1]]},
    {normal:[0,-1,0], corners:[[-1,-1,-1],[-1,-1,1],[1,-1,1],[1,-1,-1]]},
    {normal:[0,1,0], corners:[[-1,1,-1],[1,1,-1],[1,1,1],[-1,1,1]]}
  ];
  for(const chunk of chunks) {
    const tint = materialColor(chunk.material);
    for(const face of faces) {
      const start = positions.length/3;
      for(const corner of face.corners) {
        for(let axis = 0; axis < 3; axis++) {
          positions.push(chunk.center[axis] + corner[axis]*chunk.size[axis]/2);
        }
        colors.push(...tint);
      }
      indices.push(start,start + 1,start + 2,start,start + 2,start + 3);
    }
  }
  return {positions, indices, colors, metadata:{chunkCount:chunks.length, closedInteriorSurfaces:true}};
}

function materialColor(material:string) {
  switch(material) {
    case 'frame': return [0.18,0.22,0.26];
    case 'interior': return [0.42,0.2,0.08];
    default: return [0.94,0.32,0.035];
  }
}
