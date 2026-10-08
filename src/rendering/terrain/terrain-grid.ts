// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * One indexed regular lattice shared by every patch and all supported LODs.
 * Coordinates are sample offsets. Diagonals match offline error measurement.
 * @param cells
 */
export function createTerrainGrid(cells: number) {
  // Keep the finest lattice resident. Coarser levels reference subsets of the
  // same vertices, so a topology change needs only a different index range.
  const positions = new Float32Array((cells + 1)**2*2);
  for(let z = 0; z <= cells; z++) {
    for(let x = 0; x <= cells; x++) {
      positions.set([x,z],(z*(cells + 1) + x)*2);
    }
  }

  // Append one contiguous range per power-of-two stride; range.first is an
  // index offset, not a byte offset. Selection uses floor(fractional LOD).
  const indices = [], ranges = [];
  for(let stride = 1; stride <= cells; stride*= 2) {
    const first = indices.length;
    for(let z = 0; z < cells; z+= stride) {
      for(let x = 0; x < cells; x+= stride) {
        const a = z*(cells + 1) + x, b = a + stride;
        const c = a + stride*(cells + 1), d = c + stride;
        // Split each cell along a-to-d, matching height sampling and offline
        // error measurement. A different diagonal changes the rendered surface.
        indices.push(a,d,b,a,c,d);
      }
    }
    ranges.push({first,count:indices.length - first});
  }
  return {positions,indices:new Uint32Array(indices),ranges};
}
