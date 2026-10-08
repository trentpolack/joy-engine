// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { MeshInstance } from './mesh-instance.ts';
import type { MeshAsset } from './mesh-asset.ts';

/** Column-major affine transform: scale, rotate X/Y/Z, then translate.
 * @param instance
 */
export function meshTransform(instance: MeshInstance) {
  const [rx, ry, rz] = instance.rotation;
  const sx = Math.sin(rx), cx = Math.cos(rx);
  const sy = Math.sin(ry), cy = Math.cos(ry);
  const sz = Math.sin(rz), cz = Math.cos(rz);
  const [x, y, z] = instance.scale;
  return [cy*cz*x, cy*sz*x, -sy*x,
    (sx*sy*cz - cx*sz)*y, (sx*sy*sz + cx*cz)*y, sx*cy*y,
    (cx*sy*cz + sx*sz)*z, (cx*sy*sz - sx*cz)*z, cx*cy*z,
    ...instance.position];
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
export function outsideFrustum(asset: MeshAsset, transform: readonly number[], matrix: ArrayLike<number>, webgpu: boolean) {
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
