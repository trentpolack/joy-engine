// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { GPU_BACKEND } from '../gpu/constants.ts';

/**
 * Build a column-major orthographic view-projection matrix for a fixed camera
 * looking down the negative Z axis. X/Y use caller-owned world units and +Z
 * points toward the camera. Backing-store density is intentionally absent:
 * callers provide the logical viewport so display DPI cannot change framing.
 *
 * Near and far are positive distances measured forward from the camera. The
 * returned projection maps those distances to the native clip-depth range of
 * the selected luma.gl backend, producing equivalent depth-buffer values.
 *
 * @param width Logical viewport width, greater than zero.
 * @param height Logical viewport height, greater than zero.
 * @param center World-space point at viewport center.
 * @param camera Camera distances in world units.
 * @param backend luma.gl device type; `webgpu` selects [0, 1] clip depth.
 * @returns Sixteen values suitable for a column-major mat4 uniform.
 */
export function createTopDownOrthographicMatrix(width: number, height: number, center: {
    x: number;
    y: number;
}, camera: {
    height: number;
    near: number;
    far: number;
}, backend: string): number[] {
  if(width <= 0 || height <= 0) {
    throw new RangeError('Logical viewport dimensions must be positive.');
  }

  const depthRange = camera.far - camera.near;
  if(depthRange <= 0) {
    throw new RangeError('Camera far distance must be greater than its near distance.');
  }

  const depthScale = backend === GPU_BACKEND.WEBGPU ? 1 : 2;
  const depthOffset = backend === GPU_BACKEND.WEBGPU ? 0 : -1;
  return [
    2/width, 0, 0, 0,
    0, -2/height, 0, 0,
    0, 0, -depthScale/depthRange, 0,
    (-center.x*2)/width,
    (center.y*2)/height,
    ((camera.height - camera.near)*depthScale/depthRange) + depthOffset,
    1
  ];
}

/**
 * Return a back-to-front copy of interleaved triangle vertices for a fixed
 * top-down camera. Each vertex is x/y/z/R/G/B/A, so a triangle occupies 21
 * values. Smaller world Z is farther from a camera looking down -Z.
 *
 * Centroid ordering is appropriate for parallel effect planes. It is not a
 * general solution for intersecting transparent meshes or rotating cameras.
 * Equal-depth triangles retain authored order through JavaScript's stable sort.
 *
 * @param vertices Complete interleaved triangles.
 * @returns Independently owned, depth-sorted vertex data.
 */
export function sortTopDownTransparentTriangles(vertices: readonly number[]): number[] {
  if(vertices.length % 21 !== 0) {
    throw new RangeError('Triangle data must contain 21 values per triangle.');
  }
  const triangles = [];
  for(let offset = 0; offset < vertices.length; offset+= 21) {
    triangles.push({ offset, depth: (vertices[offset + 2] + vertices[offset + 9] + vertices[offset + 16])/3 });
  }
  triangles.sort((first, second) => first.depth - second.depth);

  const sorted = [];
  for(const { offset } of triangles) {
    for(let component = 0; component < 21; component+= 1) {
      sorted.push(vertices[offset + component]);
    }
  }
  return sorted;
}
