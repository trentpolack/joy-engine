// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { OrbitCamera } from 'joy-engine';
import type { GeometryData } from 'joy-engine/form';

import { defaultMaterial } from 'joy-engine/form';
import { pointBatch } from './batches.ts';

/** CPU inspection data, drawn through the viewport's existing GPU marker pass. */
export class InspectionOverlay {
  declare locations: number[][];
  declare colors: number[][] | null;
  declare selected: number;

  constructor() {

    this.locations = [];

    this.colors = null;
    this.selected = -1;
  }
  /** @param locations @param colors */
  set(locations: number[][], colors: number[][] | null) {
    this.locations = locations;
    this.colors = colors;
    this.selected = -1;
  }
  /** @param camera @param height */
  markers(camera: OrbitCamera, height: number) {

    const data: GeometryData = {positions: [], colors: [], alphas: [], points: [], pointMaterials: [],
      normals: [], uvs: [], triangles: [], triangleMaterials: [], parameters: [], project: null, materials: [defaultMaterial()]};
    const backward = camera.basis().backward;
    const step = Math.max(1, Math.ceil(this.locations.length/12000));
    if(this.colors) {
      for(let index = 0; index < this.locations.length; index+= step) {
        data.points.push(data.points.length);
        data.positions.push(...this.locations[index].map((value, axis) => value + backward[axis]*camera.radius*0.002));
        data.colors.push(...this.colors[index]);
        data.alphas.push(1);
        data.pointMaterials.push(0);
      }
    }
    const markers = pointBatch(data, false, camera, 3, height);
    const location = this.locations[this.selected];
    if(location) {
      data.positions = location.map((value, axis) => value + backward[axis]*camera.radius*0.005);
      data.colors = [1, 0.95, 0.82];
      data.alphas = [1];
      data.points = [0];
      data.pointMaterials = [0];
      markers.push(...pointBatch(data, false, camera, 7, height));
    }
    return markers;
  }
  /** Screen-space nearest element picking; coincident face corners choose the first.
   * @param x @param y @param width @param height
   * @param camera @param webgpu
   */
  pick(x: number, y: number, width: number, height: number, camera: OrbitCamera, webgpu: boolean) {
    const matrix = camera.matrix(width/height, webgpu);
    let closest = 12*12, selected = -1, nearestDepth = Infinity;
    for(let index = 0; index < this.locations.length; index++) {
      const [px, py, pz] = this.locations[index];
      const w = matrix[3]*px + matrix[7]*py + matrix[11]*pz + matrix[15];
      if(w <= 0) {
        continue;
      }
      const clipZ = (matrix[2]*px + matrix[6]*py + matrix[10]*pz + matrix[14])/w;
      if(clipZ < (webgpu ? 0 : -1) || clipZ > 1) {
        continue;
      }
      const sx = ((matrix[0]*px + matrix[4]*py + matrix[8]*pz + matrix[12])/w + 1)*width/2;
      const sy = (1 - (matrix[1]*px + matrix[5]*py + matrix[9]*pz + matrix[13])/w)*height/2;
      const distance = (x - sx)**2 + (y - sy)**2;
      if(distance < closest || (Math.abs(distance - closest) < 0.01 && w < nearestDepth)) {
        closest = distance;
        nearestDepth = w;
        selected = index;
      }
    }
    return selected;
  }
}
