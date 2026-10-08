// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { Parameter } from './parameters.ts';
import type { NamedOutput } from './procedural-output.ts';
import type { ProjectMetadata } from './parser.ts';
import type { Material } from './materials.ts';
import type { UserData } from './user-data.ts';

import { UserDataStore } from './user-data.ts';
export type { Parameter };
export interface GeometryData {
  userData?: (UserData | null)[];
  outputs?: NamedOutput[];
  project: ProjectMetadata | null;
  positions: number[];
  normals: number[];
  uvs: number[];
  colors: number[];
  alphas: number[];
  triangles: number[];
  points: number[];
  materials: Material[];
  triangleMaterials: number[];
  pointMaterials: number[];
  parameters: Parameter[];
}

/** An owned, indexed Y-up geometry builder. All distances use script units. */
export class GeometryBuilder {
  declare userDataStore: UserDataStore;
  declare userData: UserData | null;
  declare data: GeometryData;
  declare materialIndex: number;
  declare color: number[];
  declare uv: number[];
  declare normal: number[];

  /** @param [userDataStore] Shared evaluation budget for named output copies. */
  constructor(userDataStore: UserDataStore = new UserDataStore()) {
    this.userDataStore = userDataStore;

    this.userData = null;

    this.data = {
      project: null,
      materials: [],
      triangleMaterials: [],
      pointMaterials: [],
      positions: [],
      normals: [],
      uvs: [],
      colors: [],
      alphas: [],
      triangles: [],
      points: [],
      parameters: [],
    };
    this.materialIndex = 0;
    this.color = [0.929, 0.424, 0.224, 1];
    this.uv = [0, 0];
    // Zero denotes an unauthored direction: each triangle uses its own flat normal.
    this.normal = [0, 0, 0];
  }
  /** Append a vertex with copies of the active attributes; return its zero-based index.
   * @param x @param y @param z
   */
  vertex(x: number, y: number, z: number) {
    if(this.data.positions.length >= 150000 * 3) {
      throw new Error('Vertex limit exceeded (150,000).');
    }
    if(
      ![x, y, z].every(
        (value) => Number.isFinite(value) && Math.abs(value) <= 1e6,
      )
    ) {
      throw new Error('Coordinates must be finite and within ±1,000,000.');
    }
    const index = this.data.positions.length / 3;
    if(this.userData && !this.data.userData) {
      this.data.userData = Array(index).fill(null);
    }
    if(this.data.userData) {
      this.data.userData.push(((this.userDataStore.copy(this.userData)) as UserData | null));
    }
    this.data.positions.push(x, y, z);
    this.data.normals.push(...this.normal);
    this.data.uvs.push(...this.uv);
    this.data.colors.push(...this.color.slice(0, 3));
    this.data.alphas.push(this.color[3]);
    return index;
  }
  /** Append caller-supplied vertex indices using the currently selected material.
   * @param a @param b @param c
   */
  face(a: number, b: number, c: number) {
    if(this.data.triangles.length >= 180000 * 3) {
      throw new Error('Triangle limit exceeded (180,000).');
    }
    this.data.triangles.push(a, b, c);
    this.data.triangleMaterials.push(this.materialIndex);
  }
  /** Append a centered box from XYZ center and full XYZ extents in script units.
   * @param values
   */
  box(values: number[]) {
    const [x, y, z, sx, sy, sz] = values;
    if(sx <= 0 || sy <= 0 || sz <= 0) {
      throw new Error('Box sizes must be positive.');
    }
    const first = this.data.positions.length / 3;
    for(const [dx, dy, dz] of [
      [-1, -1, -1],
      [1, -1, -1],
      [1, 1, -1],
      [-1, 1, -1],
      [-1, -1, 1],
      [1, -1, 1],
      [1, 1, 1],
      [-1, 1, 1],
    ]) {
      this.vertex(x + (dx * sx) / 2, y + (dy * sy) / 2, z + (dz * sz) / 2);
    }
    for(const [a, b, c] of [
      [0, 2, 1],
      [0, 3, 2],
      [4, 5, 6],
      [4, 6, 7],
      [0, 1, 5],
      [0, 5, 4],
      [3, 7, 6],
      [3, 6, 2],
      [0, 4, 7],
      [0, 7, 3],
      [1, 2, 6],
      [1, 6, 5],
    ]) {
      this.face(first + a, first + b, first + c);
    }
  }
  /** Append a sphere from XYZ center, radius and optional longitudinal segment count.
   * Updates the active UV while generating vertices.
   * @param values
   */
  sphere(values: number[]) {
    const [x, y, z, radius, segments = 20] = values;
    integerRange(segments, 3, 96);
    if(radius <= 0) {
      throw new Error('Sphere radius must be positive.');
    }
    const first = this.data.positions.length / 3;
    const rows = Math.ceil(segments / 2);
    for(let j = 0; j <= rows; j++) {
      const latitude = (Math.PI * j) / rows;
      for(let i = 0; i <= segments; i++) {
        const angle = (Math.PI * 2 * i) / segments;
        this.uv = [i / segments, j / rows];
        this.vertex(
          x + radius * Math.sin(latitude) * Math.cos(angle),
          y + radius * Math.cos(latitude),
          z + radius * Math.sin(latitude) * Math.sin(angle),
        );
      }
    }
    for(let j = 0; j < rows; j++) {
      for(let i = 0; i < segments; i++) {
        const a = first + j * (segments + 1) + i,
          b = a + segments + 1;
        if(j > 0) {
          this.face(a, a + 1, b);
        }
        if(j < rows - 1) {
          this.face(a + 1, b + 1, b);
        }
      }
    }
  }
}
/** @param value @param min @param max */
export function integerRange(value: number, min: number, max: number) {
  if(!Number.isInteger(value) || value < min || value > max) {
    throw new Error(`Expected an integer between ${min} and ${max}.`);
  }
}

