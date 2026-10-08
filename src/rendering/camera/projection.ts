// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

export interface ViewportRect {
  left: number;
  top: number;
  width: number;
  height: number;
}
export interface WorldRay {
  origin: number[];
  direction: number[];
}

/** Project a world position to canvas-local CSS pixels. Behind-camera points return null.
 * @param position @param matrix
 * @param viewport
 */
export function projectToViewport(position: readonly number[], matrix: ArrayLike<number>, viewport: ViewportRect) {
  if(viewport.width <= 0 || viewport.height <= 0) {
    return null;
  }
  const clip = multiplyPoint(matrix, [...position, 1]);
  if(clip[3] <= 0 || !clip.every(Number.isFinite)) {
    return null;
  }
  return {x: (clip[0]/clip[3]*0.5 + 0.5)*viewport.width,
    y: (0.5 - clip[1]/clip[3]*0.5)*viewport.height};
}

/** Construct a normalized world ray from client CSS coordinates using the actual projection.
 * Origin lies on the near plane; supports perspective and orthographic matrices.
 * @param clientX @param clientY @param matrix
 * @param viewport @param [webgpu] Zero-to-one clip depth when true.
 */
export function screenRay(clientX: number, clientY: number, matrix: ArrayLike<number>, viewport: ViewportRect, webgpu: boolean = false): WorldRay | null {
  if(viewport.width <= 0 || viewport.height <= 0) {
    return null;
  }
  const inverse = invertMatrix(matrix);
  if(!inverse) {
    return null;
  }
  const x = (clientX - viewport.left)/viewport.width*2 - 1;
  const y = 1 - (clientY - viewport.top)/viewport.height*2;
  const near = multiplyPoint(inverse, [x, y, webgpu ? 0 : -1, 1]);
  const far = multiplyPoint(inverse, [x, y, 1, 1]);
  if(!near[3] || !far[3]) {
    return null;
  }
  const origin = near.slice(0, 3).map(value => value/near[3]);
  const delta = far.slice(0, 3).map((value, axis) => value/far[3] - origin[axis]);
  const length = Math.hypot(...delta);
  if(!length || !Number.isFinite(length) || !origin.every(Number.isFinite)) {
    return null;
  }
  return {origin, direction: delta.map(value => value/length)};
}

/** Intersect a forward ray with dot(normal, point) + offset = 0 in world units.
 * @param ray @param normal @param offset
 */
export function intersectRayPlane(ray: WorldRay | null, normal: readonly number[], offset: number) {
  if(!ray) {
    return null;
  }
  const denominator = dot(normal, ray.direction);
  if(Math.abs(denominator) < 1e-8) {
    return null;
  }
  const distance = -(dot(normal, ray.origin) + offset)/denominator;
  if(distance < 0 || !Number.isFinite(distance)) {
    return null;
  }
  return ray.origin.map((value, axis) => value + ray.direction[axis]*distance);
}

/** @param a @param b */
function dot(a: readonly number[], b: readonly number[]) {
  return a[0]*b[0] + a[1]*b[1] + a[2]*b[2];
}
/** @param matrix @param point */
function multiplyPoint(matrix: ArrayLike<number>, point: readonly number[]) {
  return [0, 1, 2, 3].map(row => matrix[row]*point[0] + matrix[row + 4]*point[1] + matrix[row + 8]*point[2] + matrix[row + 12]*point[3]);
}
/** Pivoted Gauss-Jordan inversion; small camera matrices prioritize clarity and stability.
 * @param matrix */
export function invertMatrix(matrix: ArrayLike<number>): number[] | null {
  if(matrix.length !== 16) {
    return null;
  }
  const rows = [0, 1, 2, 3].map(row => [0, 1, 2, 3].map(column => matrix[column*4 + row])
    .concat([0, 1, 2, 3].map(column => Number(row === column))));
  for(let column = 0; column < 4; column++) {
    let pivot = column;
    for(let row = column + 1; row < 4; row++) {
      if(Math.abs(rows[row][column]) > Math.abs(rows[pivot][column])) {
        pivot = row;
      }
    }
    const divisor = rows[pivot][column];
    if(!Number.isFinite(divisor) || Math.abs(divisor) < 1e-15) {
      return null;
    }
    [rows[column], rows[pivot]] = [rows[pivot], rows[column]];
    for(let entry = 0; entry < 8; entry++) {
      rows[column][entry]/= divisor;
    }
    for(let row = 0; row < 4; row++) {
      if(row === column) {
        continue;
      }
      const factor = rows[row][column];
      for(let entry = 0; entry < 8; entry++) {
        rows[row][entry]-= factor*rows[column][entry];
      }
    }
  }
  return [0, 1, 2, 3].flatMap(column => rows.map(row => row[column + 4]));
}
