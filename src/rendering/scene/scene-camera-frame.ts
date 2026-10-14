// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

const CAMERA_MATRIX_LENGTH = 16;
const CAMERA_MATRIX_AXIS_LENGTH = 4;
const JITTER_SAMPLE_COUNT = 8;
const JITTER_X_BASE = 2;
const JITTER_Y_BASE = 3;

/**
 * Multiply two column-major camera matrices into a new snapshot.
 * Neither borrowed input is mutated. The caller owns the returned matrix.
 * @param leftMatrix Left-hand matrix in column-major order.
 * @param rightMatrix Right-hand matrix in column-major order.
 * @returns A new column-major matrix containing leftMatrix * rightMatrix.
 */
export function multiplyCameraMatrices(
  leftMatrix: ArrayLike<number>,
  rightMatrix: ArrayLike<number>): Float32Array<ArrayBuffer> {
  const outputMatrix = new Float32Array(CAMERA_MATRIX_LENGTH);

  for(let column = 0; column < CAMERA_MATRIX_AXIS_LENGTH; column++) {
    for(let row = 0; row < CAMERA_MATRIX_AXIS_LENGTH; row++) {
      for(let axis = 0; axis < CAMERA_MATRIX_AXIS_LENGTH; axis++) {
        const outputIndex = (column*CAMERA_MATRIX_AXIS_LENGTH) + row;
        const leftIndex = (axis*CAMERA_MATRIX_AXIS_LENGTH) + row;
        const rightIndex = (column*CAMERA_MATRIX_AXIS_LENGTH) + axis;

        outputMatrix[outputIndex]+= leftMatrix[leftIndex]*rightMatrix[rightIndex];
      }
    }
  }

  return outputMatrix;
}

/**
 * Calculate the frame's repeating Halton projection jitter in top-left UV units.
 * The returned tuple is a new value scaled to the supplied backing dimensions.
 * @param frameIndex Zero-based rendered frame index.
 * @param backingWidth Render-target width in physical pixels.
 * @param backingHeight Render-target height in physical pixels.
 * @returns Horizontal and vertical projection jitter in top-left UV units.
 */
export function sceneCameraJitter(
  frameIndex: number,
  backingWidth: number,
  backingHeight: number): [number, number] {
  const sampleIndex = (frameIndex % JITTER_SAMPLE_COUNT) + 1;
  const horizontalJitter = (halton(sampleIndex, JITTER_X_BASE) - 0.5)/backingWidth;
  const verticalJitter = (halton(sampleIndex, JITTER_Y_BASE) - 0.5)/backingHeight;

  return [horizontalJitter, verticalJitter];
}

/**
 * Offset perspective clip XY by a top-left UV jitter while preserving homogeneous clip depth.
 * The borrowed projection is unchanged. The caller owns the returned matrix snapshot.
 * @param projection Column-major perspective projection matrix.
 * @param jitter Horizontal and vertical offset in top-left UV units.
 * @returns A copied projection matrix containing the requested jitter.
 */
export function jitterProjection(
  projection: Float32Array<ArrayBuffer>,
  jitter: readonly [number, number]): Float32Array<ArrayBuffer> {
  const outputProjection = projection.slice();

  for(let column = 0; column < CAMERA_MATRIX_AXIS_LENGTH; column++) {
    const columnOffset = column*CAMERA_MATRIX_AXIS_LENGTH;
    const homogeneousDepth = projection[columnOffset + 3];

    outputProjection[columnOffset]+= 2*jitter[0]*homogeneousDepth;
    outputProjection[columnOffset + 1]-= 2*jitter[1]*homogeneousDepth;
  }

  return outputProjection;
}

/**
 * Calculate one value from a low-discrepancy Halton sequence.
 * @param index Positive sequence index.
 * @param base Prime base that identifies the sequence dimension.
 * @returns The sequence value in the half-open interval [0, 1).
 */
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
