// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {createSeededRandom} from 'joy-engine';
import type {DestructibleChunkDefinition} from 'joy-engine';
import {DESTRUCTION_CONFIG} from './config/destruction-config.ts';

/**
 * Generate stable wall and return-shell chunks. The seed affects materials, not identities or collision.
 */
export function generateStructure(seed:number = DESTRUCTION_CONFIG.seed) {
  const random = createSeededRandom(seed);
  const chunks:DestructibleChunkDefinition[] = [];
  const [width,height,depth] = DESTRUCTION_CONFIG.chunkSize;
  const halfColumns = (DESTRUCTION_CONFIG.wallColumns - 1)/2;

  for(let row = 0; row < DESTRUCTION_CONFIG.wallRows; row++) {
    for(let column = 0; column < DESTRUCTION_CONFIG.wallColumns; column++) {
      chunks.push({id:`front:${column}:${row}`,center:[(column - halfColumns)*width,row*height + height/2,0],
        size:[width,height,depth],material:random() > 0.22 ? 'masonry' : 'interior',hitPoints:DESTRUCTION_CONFIG.hitPoints});
    }
  }

  // A shallow U-shaped shell exposes return walls when inspecting a damaged front face.
  for(let row = 0; row < 4; row++) {
    for(let depthIndex = 1; depthIndex <= 6; depthIndex++) {
      for(const side of [-1,1]) {
        chunks.push({id:`side:${side}:${depthIndex}:${row}`,center:[side*(halfColumns*width),row*height + height/2,-depthIndex*depth],
          size:[depth,height,depth],material:'frame',hitPoints:DESTRUCTION_CONFIG.frameHitPoints});
      }
    }
  }
  return chunks;
}
