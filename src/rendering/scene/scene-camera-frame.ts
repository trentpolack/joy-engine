// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/** Multiply column-major camera matrices into a caller-owned float32 snapshot. */
export function multiplyCameraMatrices(a: ArrayLike<number>, b: ArrayLike<number>): Float32Array<ArrayBuffer> {
  const result = new Float32Array(16);
  for(let column = 0; column < 4; column++) {
    for(let row = 0; row < 4; row++) {
      for(let axis = 0; axis < 4; axis++) {
        result[column*4 + row]+= a[axis*4 + row]*b[column*4 + axis];
      }
    }
  }
  return result;
}

/** Eight-sample Halton projection jitter in top-left UV units, scaled to backing pixels. */
export function sceneCameraJitter(frameIndex: number, width: number, height: number): [number, number] {
  const sample = (frameIndex % 8) + 1;
  return [(halton(sample, 2) - 0.5)/width, (halton(sample, 3) - 0.5)/height];
}

/** Offset perspective clip XY by a UV jitter, preserving homogeneous clip depth. */
export function jitterProjection(projection: Float32Array<ArrayBuffer>, jitter: readonly [number, number]): Float32Array<ArrayBuffer> {
  const result = projection.slice();
  for(let column = 0; column < 4; column++) {
    result[column*4]+= 2*jitter[0]*projection[column*4 + 3];
    result[column*4 + 1]-= 2*jitter[1]*projection[column*4 + 3];
  }
  return result;
}

function halton(index: number, base: number): number {
  let scale = 1;
  let result = 0;
  while(index > 0) {
    scale/= base;
    result+= scale*(index % base);
    index = Math.floor(index/base);
  }
  return result;
}
