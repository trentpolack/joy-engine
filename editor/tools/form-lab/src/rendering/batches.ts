// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { MATERIAL_ALPHA_MODE } from 'joy-engine/constants';
import type { GeometryData, Material } from 'joy-engine/form';
import type { OrbitCamera } from 'joy-engine';

export type { GeometryData };

/** Camera-facing point markers have a constant pixel footprint for a fixed camera depth.
 * @param data @param allVertices @param camera @param size @param viewportHeight
 */
export function pointBatch(data: GeometryData, allVertices: boolean, camera: OrbitCamera, size: number, viewportHeight: number) {

  const out: number[] = [];
  const { right, up } = camera.basis();
  const assignments = new Map();
  if(allVertices) {
    data.triangles.forEach((index, i) => assignments.set(index, data.triangleMaterials[Math.floor(i / 3)]));
    data.points.forEach((index, i) => assignments.set(index, data.pointMaterials[i]));
  }
  const count = allVertices ? data.positions.length / 3 : data.points.length;
  // Full data is retained for export; cap preview markers to keep orbit responsive.
  const stride = Math.max(1, Math.ceil(count / 40000));
  const radius =
    (camera.distance * Math.tan(Math.PI / 8) * size) /
    Math.max(100, viewportHeight);
  for(let i = 0; i < count; i += stride) {
    const index = allVertices ? i : data.points[i];
    const material = data.materials[allVertices ? assignments.get(index) ?? 0 : data.pointMaterials[i]];
    const position = data.positions.slice(index * 3, index * 3 + 3);
    const color = data.colors.slice(index * 3, index * 3 + 3).map((value, channel) => value * material.baseColor[channel] + material.emissive[channel]);
    const corners = [
      [-1, -1],
      [1, -1],
      [1, 1],
      [-1, 1],
    ].map(([x, y]) =>
      position.map((value, j) => value + (right[j] * x + up[j] * y) * radius),
    );
    for(const corner of [0, 1, 2, 0, 2, 3]) {
      appendVertex(out, corners[corner], color, materialAlpha(material, data.alphas[index]));
    }
  }
  return out;
}
/** @param out @param position @param color @param [alpha] */
function appendVertex(out: number[], position: number[], color: number[], alpha: number = 1) {
  out.push(...position, ...color, alpha);
}
/** Alpha follows the same material policy as glTF, including vertex alpha.
 * @param material @param vertexAlpha
 */
function materialAlpha(material: Material, vertexAlpha: number) {
  const alpha = material.baseColor[3] * vertexAlpha;
  if(material.alphaMode === MATERIAL_ALPHA_MODE.OPAQUE) {
    return 1;
  }
  if(material.alphaMode === MATERIAL_ALPHA_MODE.MASK) {
    return alpha >= material.alphaCutoff ? 1 : 0;
  }
  return alpha;
}

