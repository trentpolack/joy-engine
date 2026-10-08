// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { TerrainAsset } from './terrain-asset.ts';

import { sampleHeightTriangle } from './terrain-asset.ts';
export type { TerrainAsset };
export interface TerrainView {
  eye: number[];
  projectionScale: number;
  tolerance: number;
  target: number[];
  targetRadius: number;
}
export interface TerrainSelection {
  detail: number;
  lod: number;
  edges: number[];
}

/**
 * Validate shared CPU/compute inputs before any resource upload.
 * @param view
 */
export function validateTerrainView(view: TerrainView) {
  /** @param value */
  const vector = (value: number[]) => Array.isArray(value) && value.length === 3
    && [0,1,2].every(i => Number.isFinite(value[i]));
  if(!vector(view.eye) || !vector(view.target)
    || !Number.isFinite(view.projectionScale) || view.projectionScale <= 0
    || !Number.isFinite(view.tolerance) || view.tolerance <= 0
    || !Number.isFinite(view.targetRadius) || view.targetRadius < 0) {
    throw new Error('Invalid terrain view.');
  }
}

/**
 * Select fixed quadtree leaves using conservative AABB distance and prebuilt
 * geometric errors. projectionScale is CSS viewport height/(2*tan(vertical FOV/2)).
 * Max cell size is 32 CSS pixels; target distance is horizontal world units.
 * Returns bounded descriptors; no authored heights are scanned here.
 * @param asset @param view
 */
export function selectTerrain(asset: TerrainAsset, view: TerrainView): TerrainSelection[] {
  validateTerrainView(view);
  const {size,tileCells,extent} = asset.manifest;
  // Use the larger sample spacing so rectangular world extents respect the
  // screen-space cell limit along both axes. No height scan is needed per frame.
  const spacing = Math.max(...extent)/(size - 1);
  const details = asset.patches.map(patch => {
    // Convert the fixed leaf footprint to a world-space box. Distance to the
    // nearest point is conservative: a large nearby patch cannot hide behind
    // a distant center when deciding how much detail it needs.
    const min = [patch.x/(size - 1)*extent[0] - extent[0]/2,patch.min,patch.z/(size - 1)*extent[1] - extent[1]/2];
    const max = [min[0] + tileCells/(size - 1)*extent[0],patch.max,min[2] + tileCells/(size - 1)*extent[1]];
    // The target pin uses horizontal distance to the footprint, independent of
    // target elevation, so patches touching the protected region stay finest.
    const dx = Math.max(min[0] - view.target[0],0,view.target[0] - max[0]);
    const dz = Math.max(min[2] - view.target[2],0,view.target[2] - max[2]);
    if(view.targetRadius > 0 && Math.hypot(dx,dz) < view.targetRadius) {
      return 0;
    }
    const distance = Math.max(0.01,Math.hypot(...min.map((v,i) => Math.max(v - view.eye[i],0,view.eye[i] - max[i]))));

    // Convert the allowed pixel error into world units at the nearest distance.
    // The extra error floor limits projected cell size even on perfectly flat
    // terrain, where geometric error alone would allow very large triangles.
    const budget = distance/view.projectionScale*view.tolerance;
    const errors = patch.errors.map((error,level) => Math.max(error,level === 0 ? 0 : 2**level*spacing*view.tolerance/32));

    // Larger levels mean coarser grids. Keep the last level within budget, then
    // use the remaining budget as a morph fraction toward its parent surface.
    let level = 0;
    while(level < asset.maxLod && errors[level + 1] <= budget) {
      level+= 1;
    }
    const detail = level === asset.maxLod ? level : level + Math.max(0,Math.min(1,(budget - errors[level])/(errors[level + 1] - errors[level])));
    // The pin fades over one radius, so crossing its boundary cannot pop topology.
    const fade = view.targetRadius > 0 ? Math.max(0,Math.min(1,(Math.hypot(dx,dz) - view.targetRadius)/view.targetRadius)) : 1;
    return detail*fade*fade*(3 - 2*fade);
  });
  return reconcileTerrainEdges(asset, details);
}

/**
 * Both sides use the coarser fractional lattice, including its continuous morph.
 * Endpoints are authored tile corners at all supported levels, so four-way
 * corners remain invariant. Arbitrary neighbor ratios are supported.
 * @param asset @param details
 */
export function reconcileTerrainEdges(asset: TerrainAsset, details: number[]): TerrainSelection[] {
  const axis = asset.tilesPerAxis;
  return details.map((detail,i) => {
    // Patches are row-major. Both sides choose max(detail), so their boundary
    // heights agree even when their interior grids differ by several levels.
    // Edge slots are -X, +X, -Z, +Z; outside edges keep the patch's own level.
    const x = i % axis, z = Math.floor(i/axis);
    return {detail,lod:Math.floor(detail),edges:[
      Math.max(detail,x > 0 ? details[i - 1] : detail),
      Math.max(detail,x < axis - 1 ? details[i + 1] : detail),
      Math.max(detail,z > 0 ? details[i - axis] : detail),
      Math.max(detail,z < axis - 1 ? details[i + axis] : detail)
    ]};
  });
}

/**
 * CPU reference for shader parity and seam validation; local coordinates in samples.
 * @param asset @param patchIndex @param x @param z @param selection
 */
export function sampleMorphedHeight(asset: TerrainAsset, patchIndex: number, x: number, z: number, selection: TerrainSelection) {
  const cells = asset.manifest.tileCells;
  // Boundaries follow the shared edge level; at corners take the coarsest
  // participating edge. Tile corners are retained samples at every stride.
  let detail = selection.detail;
  if(x === 0) { detail = selection.edges[0]; }
  if(x === cells) { detail = selection.edges[1]; }
  if(z === 0) { detail = Math.max(detail,selection.edges[2]); }
  if(z === cells) { detail = Math.max(detail,selection.edges[3]); }
  const level = Math.floor(detail), stride = 2**level;
  const patch = asset.patches[patchIndex];
  /** @param px @param pz @param d */

  // Fractional detail blends two triangle surfaces, rather than bilinear
  // texture samples. This is the same diagonal used by the indexed mesh.
  const height = (px: number,pz: number,d: number) => {
    const level = Math.floor(d), step = 2**level;
    const fine = sampleHeightTriangle(asset,patch.x + px,patch.z + pz,step);
    const coarse = sampleHeightTriangle(asset,patch.x + px,patch.z + pz,Math.min(cells,step*2));
    return fine + (coarse - fine)*(d - level);
  };
  if(x === 0 || x === cells || z === 0 || z === cells) { return height(x,z,detail); }
  /** @param px @param pz @param step */

  // Parent boundary vertices must already include neighbor stitching. Using
  // raw heights here would morph interior vertices toward the wrong surface.
  const vertex = (px: number,pz: number,step: number) => {
    let edge = Math.log2(step);
    if(px === 0) { edge = Math.max(edge,selection.edges[0]); }
    if(px === cells) { edge = Math.max(edge,selection.edges[1]); }
    if(pz === 0) { edge = Math.max(edge,selection.edges[2]); }
    if(pz === cells) { edge = Math.max(edge,selection.edges[3]); }
    return height(px,pz,edge);
  };
  /** @param step */

  // Reconstruct the actual triangle containing the point. Clamp the cell
  // origin at the positive boundary to keep all four samples inside the tile.
  const surface = (step: number) => {
    const bx = Math.min(Math.floor(x/step)*step,cells - step);
    const bz = Math.min(Math.floor(z/step)*step,cells - step);
    const fx = (x - bx)/step, fz = (z - bz)/step;
    const a = vertex(bx,bz,step), b = vertex(bx + step,bz,step);
    const c = vertex(bx,bz + step,step), d = vertex(bx + step,bz + step,step);
    return fx >= fz ? a + (b - a)*fx + (d - b)*fz : a + (d - c)*fx + (c - a)*fz;
  };
  // Removed vertices converge to the actual parent triangles, including edges.
  const fine = surface(stride), coarse = surface(Math.min(cells,stride*2));
  return fine + (coarse - fine)*(detail - level);
}
