// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { PbrMesh } from '../../core/types.ts';

/** Copy an indexed mesh into a GPU stream, preserving hard edges unless normals are authored.
 * The returned arrays are owned by the caller. Positions and normals stay in world space.
 * @param mesh
 */
export function preparePbrGeometry(mesh: PbrMesh) {
  const count = mesh.positions.length/3;
  if(!Number.isInteger(count) || mesh.normals.length !== count*3 || mesh.colors.length !== count*3 || mesh.uvs.length !== count*2 || mesh.alphas.length !== count || mesh.triangles.length % 3 || mesh.triangleMaterials.length !== mesh.triangles.length/3) {
    throw new Error('PBR geometry requires aligned positions, normals, colors, alpha, UVs and triangle materials.');
  }
  for(const values of [mesh.positions, mesh.normals, mesh.colors, mesh.uvs, mesh.alphas]) {
    if(values.some(value => !Number.isFinite(Math.fround(value)))) {
      throw new Error('PBR vertex attributes must fit finite float32 values.');
    }
  }
  const vertices = new Float32Array(mesh.triangles.length*12);

  const triangles: {
    firstVertex: number;
    material: number;
    center: number[];
}[] = [];
  for(let offset = 0; offset < mesh.triangles.length; offset+= 3) {
    const indices = mesh.triangles.slice(offset, offset + 3);
    if(indices.some(index => !Number.isInteger(index) || index < 0 || index >= count)) {
      throw new Error('PBR triangle index is outside the vertex array.');
    }
    const material = mesh.triangleMaterials[offset/3];
    if(!Number.isInteger(material) || !mesh.materials[material]) {
      throw new Error('PBR triangle references an unknown material.');
    }
    const positions = indices.map(index => mesh.positions.slice(index*3, index*3 + 3));
    const [a, b, c] = positions;
    const ab = b.map((value, axis) => value - a[axis]);
    const ac = c.map((value, axis) => value - a[axis]);
    const face = [ab[1]*ac[2] - ab[2]*ac[1], ab[2]*ac[0] - ab[0]*ac[2], ab[0]*ac[1] - ab[1]*ac[0]];
    const faceLength = Math.hypot(...face);
    const flatNormal = faceLength > 0 ? face.map(value => value/faceLength) : [0, 1, 0];
    for(let corner = 0; corner < 3; corner++) {
      const index = indices[corner];
      const authored = mesh.normals.slice(index*3, index*3 + 3);
      const scale = Math.max(...authored.map(Math.abs));
      const scaled = scale > 0 ? authored.map(value => value/scale) : authored;
      const length = Math.hypot(...scaled);
      const normal = length > 0 ? scaled.map(value => value/length) : flatNormal;
      vertices.set([
        ...positions[corner], ...normal,
        ...mesh.colors.slice(index*3, index*3 + 3), mesh.alphas[index],
        ...mesh.uvs.slice(index*2, index*2 + 2)
      ], (offset + corner)*12);
    }
    triangles.push({firstVertex: offset, material, center: a.map((value, axis) => (value + b[axis] + c[axis])/3)});
  }
  return { vertices, triangles };
}
