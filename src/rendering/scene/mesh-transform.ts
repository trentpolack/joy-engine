// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { AffineTransform, GeometryBounds } from '../../core/types.ts';

/** Column-major affine transform: scale, rotate X/Y/Z, then translate.
 * @param instance
 */
export function meshTransform(instance: {position: readonly number[]; rotation: readonly number[]; scale: readonly number[]}): AffineTransform {
  const [rx, ry, rz] = instance.rotation;
  const sx = Math.sin(rx), cx = Math.cos(rx);
  const sy = Math.sin(ry), cy = Math.cos(ry);
  const sz = Math.sin(rz), cz = Math.cos(rz);
  const [x, y, z] = instance.scale;
  return [cy*cz*x, cy*sz*x, -sy*x,
    (sx*sy*cz - cx*sz)*y, (sx*sy*sz + cx*cz)*y, sx*cy*y,
    (cx*sy*cz + sx*sz)*z, (cx*sy*sz - sx*cz)*z, cx*cy*z,
    instance.position[0], instance.position[1], instance.position[2]];
}

/** @param matrix @param x @param y @param z */
export function transformPoint(matrix: readonly number[], x: number, y: number, z: number) {
  return [matrix[0]*x + matrix[3]*y + matrix[6]*z + matrix[9],
    matrix[1]*x + matrix[4]*y + matrix[7]*z + matrix[10],
    matrix[2]*x + matrix[5]*y + matrix[8]*z + matrix[11]];
}

/** Conservative clip-plane test of transformed local bounds, including near-plane crossings.
 * @param asset @param transform
 * @param matrix View/projection matrix. @param webgpu
 */
export function outsideFrustum(asset: {bounds: GeometryBounds}, transform: readonly number[], matrix: ArrayLike<number>, webgpu: boolean) {
  const outside = [true, true, true, true, true, true];
  for(let corner = 0; corner < 8; corner++) {
    const local = [0, 1, 2].map(axis => (corner & (1 << axis)) ? asset.bounds.max[axis] : asset.bounds.min[axis]);
    const [x, y, z] = transformPoint(transform, local[0], local[1], local[2]);
    const clip = [0, 1, 2, 3].map(row => matrix[row]*x + matrix[row + 4]*y + matrix[row + 8]*z + matrix[row + 12]);
    const [cx, cy, cz, w] = clip;
    const distances = [cx + w, w - cx, cy + w, w - cy, webgpu ? cz : cz + w, w - cz];
    for(let plane = 0; plane < 6; plane++) {
      outside[plane] = outside[plane] && distances[plane] < 0;
    }
  }
  return outside.some(Boolean);
}

/** Compose complete affine bases without decomposing shear or dividing by scale. */
export function composeTransforms(parent: AffineTransform, local: AffineTransform): AffineTransform {
  const result = Array(12).fill(0) as number[];
  for(let column = 0; column < 3; column++) {
    for(let row = 0; row < 3; row++) {
      result[column*3 + row] = parent[row]*local[column*3] + parent[3 + row]*local[column*3 + 1] + parent[6 + row]*local[column*3 + 2];
    }
  }
  const translation = transformPoint(parent, local[9], local[10], local[11]);
  return [result[0], result[1], result[2], result[3], result[4], result[5], result[6], result[7], result[8], translation[0], translation[1], translation[2]];
}

/** Transform the local bounding box into a conservative world-space bounding box. */
export function transformBounds(bounds: GeometryBounds, transform: AffineTransform): GeometryBounds {
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for(let corner = 0; corner < 8; corner++) {
    const point = transformPoint(transform,
      (corner & 1) ? bounds.max[0] : bounds.min[0],
      (corner & 2) ? bounds.max[1] : bounds.min[1],
      (corner & 4) ? bounds.max[2] : bounds.min[2]);
    for(let axis = 0; axis < 3; axis++) {
      min[axis] = Math.min(min[axis], point[axis]);
      max[axis] = Math.max(max[axis], point[axis]);
    }
  }
  return {min, max};
}
