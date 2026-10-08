// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { FORM_VALUE_KIND } from './constants.ts';
import { MATERIAL_ALPHA_MODE } from '../rendering/pbr/constants.ts';
import type { Material } from './materials.ts';
import { GeometryBuilder } from './geometry.ts';
import { defaultMaterial, readMaterials } from './materials.ts';

export interface MeshValue {
  kind: typeof FORM_VALUE_KIND.MESH;
  name: string;
  positions: number[];
  normals: number[];
  uvs: number[];
  indices: number[];
}
export interface MaterialValue {
  kind: typeof FORM_VALUE_KIND.MATERIAL;
  material: Material;
}
const MAX_VERTICES = 150000;
const MAX_INDICES = 180000*3;

/** Create reusable, centered Y-up box geometry in script units. @param width @param height @param depth */
export function meshBox(width: number, height: number, depth: number): MeshValue {
  const builder = new GeometryBuilder();
  builder.box([0, 0, 0, width, height, depth]);
  return fromBuilder('Box', builder);
}

/** Create reusable sphere geometry. @param radius @param [segments] */
export function meshSphere(radius: number, segments: number = 20): MeshValue {
  const builder = new GeometryBuilder();
  builder.sphere([0, 0, 0, radius, segments]);
  return fromBuilder('Sphere', builder);
}

/** Create an owned PBR material value. Color is linear RGBA; metallic/roughness are in [0,1].
 * @param color @param metallic @param roughness */
export function materialPbr(color: number[], metallic: number, roughness: number): MaterialValue {
  return readMaterialValue({kind: FORM_VALUE_KIND.MATERIAL, material: {
    ...defaultMaterial(), baseColor: color, metallic, roughness,
    alphaMode: color[3] < 1 ? MATERIAL_ALPHA_MODE.BLEND : MATERIAL_ALPHA_MODE.OPAQUE
  }});
}

/** Validate and copy borrowed host/serialized mesh data; never retain caller arrays.
 * Positions are model-space script units; normals may be zero for flat shading.
 * @param value */
export function readMesh(value: unknown): MeshValue {
  if(!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('Expected a mesh value.');
  }
  const mesh = ((value) as Record<string, unknown>);
  if(mesh.kind !== FORM_VALUE_KIND.MESH || typeof mesh.name !== 'string' || mesh.name.length > 120) {
    throw new Error('Mesh requires kind and a name of at most 120 characters.');
  }
  const positions = numbers(mesh.positions, MAX_VERTICES*3, 'Mesh positions');
  if(!positions.length || positions.length%3 || positions.some(v => Math.abs(v) > 1e6)) {
    throw new Error('Mesh positions require complete XYZ vertices within ±1,000,000.');
  }
  const normals = numbers(mesh.normals, MAX_VERTICES*3, 'Mesh normals');
  const uvs = numbers(mesh.uvs, MAX_VERTICES*2, 'Mesh UVs');
  const indices = numbers(mesh.indices, MAX_INDICES, 'Mesh indices');
  if(normals.length !== positions.length || uvs.length !== positions.length/3*2) {
    throw new Error('Mesh normals and UVs must match the vertex count.');
  }
  if(!indices.length || indices.length%3 || indices.some(i => !Number.isInteger(i) || i < 0 || i >= positions.length/3)) {
    throw new Error('Mesh indices must contain complete triangles with valid vertex indices.');
  }
  return {kind: FORM_VALUE_KIND.MESH, name: mesh.name, positions, normals, uvs, indices};
}

/** Validate and copy a material parameter, including existing texture budgets.
 * @param value */
export function readMaterialValue(value: unknown): MaterialValue {
  if(!value || typeof value !== 'object' || !('kind' in value) || value.kind !== FORM_VALUE_KIND.MATERIAL || !('material' in value)) {
    throw new Error('Expected a material value.');
  }
  const materials = readMaterials([value.material]);
  return {kind: FORM_VALUE_KIND.MATERIAL, material: materials[materials.length - 1]};
}

/** Return transformed geometry without changing the source. Scale, then XYZ Euler rotation in radians, then translation.
 * Normals use inverse scale; reflected transforms reverse winding. Singular scales are rejected.
 * @param mesh @param translation @param rotation @param scale */
export function transformMesh(mesh: MeshValue, translation: number[], rotation: number[], scale: number[]): MeshValue {
  for(const vector of [translation, rotation, scale]) {
    if(vector.length !== 3 || !vector.every(Number.isFinite)) {
      throw new Error('Transforms require finite XYZ vectors.');
    }
  }
  if(scale.some(v => Math.abs(v) < 1e-9)) {
    throw new Error('Mesh scale must be nonzero.');
  }
  const result = readMesh(mesh);
  const [sx, sy, sz] = scale;
  const [rx, ry, rz] = rotation;
  const cx = Math.cos(rx), ax = Math.sin(rx);
  const cy = Math.cos(ry), ay = Math.sin(ry);
  const cz = Math.cos(rz), az = Math.sin(rz);
  /** @param x @param y @param z */
  function rotate(x: number, y: number, z: number) {
    const y1 = y*cx - z*ax, z1 = y*ax + z*cx;
    const x2 = x*cy + z1*ay, z2 = -x*ay + z1*cy;
    return [x2*cz - y1*az, x2*az + y1*cz, z2];
  }
  for(let i = 0; i < result.positions.length; i+= 3) {
    const position = rotate(mesh.positions[i]*sx, mesh.positions[i + 1]*sy, mesh.positions[i + 2]*sz);
    const normal = rotate(mesh.normals[i]/sx, mesh.normals[i + 1]/sy, mesh.normals[i + 2]/sz);
    const length = Math.hypot(...normal);
    for(let j = 0; j < 3; j++) {
      result.positions[i + j] = position[j] + translation[j];
      result.normals[i + j] = length > 0 ? normal[j]/length : 0;
    }
  }
  if(sx*sy*sz < 0) {
    for(let i = 0; i < result.indices.length; i+= 3) {
      [result.indices[i + 1], result.indices[i + 2]] = [result.indices[i + 2], result.indices[i + 1]];
    }
  }
  return readMesh(result);
}

/** @param name @param builder */
function fromBuilder(name: string, builder: GeometryBuilder): MeshValue {
  const {positions, normals, uvs, triangles} = builder.data;
  return {kind: FORM_VALUE_KIND.MESH, name, positions, normals, uvs, indices: triangles};
}

/** @param value @param limit @param name */
function numbers(value: unknown, limit: number, name: string): number[] {
  if(!Array.isArray(value) || value.length > limit || !Array.from(value).every(v => typeof v === 'number' && Number.isFinite(v))) {
    throw new Error(`${name} must be finite numbers within the geometry limit.`);
  }
  return [...value];
}
