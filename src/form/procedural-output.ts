// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { FORM_VALUE_KIND } from './constants.ts';
import type { GeometryValue, ProceduralValue } from './procedural-geometry.ts';
import type { GeometryData } from './geometry.ts';
import type { MaterialValue } from './resources.ts';
import { GeometryBuilder } from './geometry.ts';
export type { GeometryValue };
export type { ProceduralValue };
export interface NamedOutput {
  name: string;
  visible: boolean;
  value: ProceduralValue;
  geometry: GeometryData;
}

/** Realize authored point/corner attributes only at the renderer/export boundary.
 * Instances remain retained in NamedOutput.value for runtime consumers.
 * @param value @param builder
 * @param bindMaterial
 */
export function emitProcedural(value: ProceduralValue, builder: GeometryBuilder, bindMaterial: (material: MaterialValue) => number) {
  if(value.kind === FORM_VALUE_KIND.GEOMETRY) {
    emitGeometry(value, builder);
    return;
  }
  const sites = value.sites.attributes.points;
  const saved = {materialIndex: builder.materialIndex, color: builder.color, userData: builder.userData};
  if(value.material) {
    builder.materialIndex = bindMaterial(value.material);
  }
  try {
    for(let index = 0; index < sites.position.values.length; index++) {
      const position = ((sites.position.values[index]) as number[]);
      const rotation = ((sites.rotation?.values[index] ?? [0, 0, 0]) as number[]);
      const scale = ((sites.scale?.values[index] ?? [1, 1, 1]) as number[]);
      const tint = ((sites.color?.values[index] ?? [1, 1, 1, 1]) as number[]);
      builder.color = saved.color.map((component, i) => component*tint[i]);
      builder.userData = value.sites.userData ? value.sites.userData[index] : saved.userData;
      if(scale.some(value => Math.abs(value) < 1e-9)) {
        throw new Error('Instance scale must be nonzero.');
      }
      emitGeometry(value.source, builder, {position, rotation, scale});
    }
  } finally {
    Object.assign(builder, saved);
  }
}

/** @param value @param builder
 * @param [transform]
 */
function emitGeometry(value: GeometryValue, builder: GeometryBuilder, transform?: {
    position: number[];
    rotation: number[];
    scale: number[];
}) {
  const points = value.attributes.points;
  const corners = value.attributes.corners;
  const faces = value.attributes.faces;
  const saved = {color: builder.color, normal: builder.normal, uv: builder.uv, userData: builder.userData};
  const used = new Set(value.faces.flat());
  const split = [...Object.keys(corners), ...Object.keys(faces)].some(name => ['normal', 'uv', 'color'].includes(name));
  /** @param point @param [corner] @param [face] */
  const vertex = (point: number, corner?: number, face?: number) => {
    /** @param name @param fallback */
    const attribute = (name: string, fallback: number[]) =>  ((
      (corner !== undefined ? corners[name]?.values[corner] : undefined) ??
      (face !== undefined ? faces[name]?.values[face] : undefined) ?? points[name]?.values[point] ?? fallback) as number[]);
    let position = ((points.position.values[point]) as number[]);
    let normal = attribute('normal', [0, 0, 0]);
    const normalScale = Math.max(...normal.map(Math.abs));
    normal = normalScale ? normal.map(component => component/normalScale) : normal;
    const normalLength = Math.hypot(...normal);
    normal = normalLength ? normal.map(component => component/normalLength) : normal;
    if(transform) {
      position = rotate(position.map((value, axis) => value*transform.scale[axis]), transform.rotation)
        .map((value, axis) => value + transform.position[axis]);
      normal = rotate(normal.map((value, axis) => value/transform.scale[axis]), transform.rotation);
      const length = Math.hypot(...normal);
      normal = normal.map(value => length ? value/length : 0);
    }
    const tint = attribute('color', [1, 1, 1, 1]);
    builder.color = saved.color.map((value, axis) => value*tint[axis]);
    builder.userData = value.userData ? value.userData[point] : saved.userData;
    builder.normal = normal;
    builder.uv = attribute('uv', [0, 0, 0]).slice(0, 2);
    return builder.vertex(position[0], position[1], position[2]);
  };
  try {
    const first = builder.data.positions.length/3;
    if(!split) {
      for(let point = 0; point < points.position.values.length; point++) {
        vertex(point);
      }
    }
    for(let face = 0; face < value.faces.length; face++) {
      const indices = value.faces[face].map((point, corner) => split ? vertex(point, face*3 + corner, face) : first + point);
      if(transform && transform.scale[0]*transform.scale[1]*transform.scale[2] < 0) {
        [indices[1], indices[2]] = [indices[2], indices[1]];
      }
      builder.face(indices[0], indices[1], indices[2]);
    }
    for(let point = 0; point < points.position.values.length; point++) {
      if(!used.has(point)) {
        builder.data.points.push(split ? vertex(point) : first + point);
        builder.data.pointMaterials.push(builder.materialIndex);
      }
    }
  } finally {
    Object.assign(builder, saved);
  }
}
/** XYZ Euler rotation in radians. @param value @param angles */
function rotate(value: number[], angles: number[]) {
  let [x, y, z] = value;
  const [a, b, c] = angles;
  [y, z] = [y*Math.cos(a) - z*Math.sin(a), y*Math.sin(a) + z*Math.cos(a)];
  [x, z] = [x*Math.cos(b) + z*Math.sin(b), -x*Math.sin(b) + z*Math.cos(b)];
  return [x*Math.cos(c) - y*Math.sin(c), x*Math.sin(c) + y*Math.cos(c), z];
}
