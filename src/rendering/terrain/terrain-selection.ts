// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {
  TerrainAsset,
  TerrainPatch
} from './terrain-asset.ts';

import {sampleHeightTriangle} from './terrain-asset.ts';

export type {TerrainAsset};

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

const VECTOR_COMPONENT_COUNT = 3;
const MINIMUM_DISTANCE = 0.01;
const MAXIMUM_CELL_PIXELS = 32;

/**
 * Validate shared CPU and compute inputs before any resource upload.
 * @param view Borrowed camera and protected-target state in world and CSS-pixel units.
 * @throws When a vector is malformed or a scalar is outside its supported range.
 */
export function validateTerrainView(view: TerrainView): void {
  const hasValidEye = isFiniteVector3(view.eye);
  const hasValidTarget = isFiniteVector3(view.target);
  const hasValidProjection = Number.isFinite(view.projectionScale) && (view.projectionScale > 0);
  const hasValidTolerance = Number.isFinite(view.tolerance) && (view.tolerance > 0);
  const hasValidTargetRadius = Number.isFinite(view.targetRadius) && (view.targetRadius >= 0);

  if(!hasValidEye
    || !hasValidTarget
    || !hasValidProjection
    || !hasValidTolerance
    || !hasValidTargetRadius) {
    throw new Error('Invalid terrain view.');
  }
}

/**
 * Select fixed quadtree leaves using conservative AABB distance and prebuilt geometric errors.
 * Projection scale is CSS viewport height divided by `2*tan(vertical FOV/2)`. Target distance is
 * measured in horizontal world units. No authored heights are scanned or mutated.
 * @param asset Borrowed resident terrain and its immutable patch hierarchy.
 * @param view Borrowed camera and protected-target state.
 * @returns New bounded descriptors with cells limited to 32 CSS pixels.
 */
export function selectTerrain(asset: TerrainAsset, view: TerrainView): TerrainSelection[] {
  validateTerrainView(view);

  const {size, extent} = asset.manifest;
  const sampleSpacing = Math.max(...extent)/(size - 1);
  const details = asset.patches.map(patch => calculatePatchDetail(asset, patch, view, sampleSpacing));

  return reconcileTerrainEdges(asset, details);
}

/**
 * Reconcile each patch with the coarser fractional lattice of its neighbors.
 * Endpoints remain authored tile corners at every supported level, so four-way corners agree.
 * @param asset Borrowed terrain whose patches use complete row-major ordering.
 * @param details Borrowed fractional LOD values in patch order.
 * @returns New per-patch descriptors with edges ordered -X, +X, -Z, +Z.
 */
export function reconcileTerrainEdges(asset: TerrainAsset, details: number[]): TerrainSelection[] {
  const patchesPerAxis = asset.tilesPerAxis;

  return details.map((detail, patchIndex) => {
    const patchX = patchIndex % patchesPerAxis;
    const patchZ = Math.floor(patchIndex/patchesPerAxis);
    const negativeX = patchX > 0 ? details[patchIndex - 1] : detail;
    const positiveX = patchX < patchesPerAxis - 1 ? details[patchIndex + 1] : detail;
    const negativeZ = patchZ > 0 ? details[patchIndex - patchesPerAxis] : detail;
    const positiveZ = patchZ < patchesPerAxis - 1 ? details[patchIndex + patchesPerAxis] : detail;

    return {
      detail,
      lod: Math.floor(detail),
      edges: [
        Math.max(detail, negativeX),
        Math.max(detail, positiveX),
        Math.max(detail, negativeZ),
        Math.max(detail, positiveZ)
      ]
    };
  });
}

/**
 * Sample the CPU reference surface used for shader parity and seam validation.
 * @param asset Borrowed resident terrain data.
 * @param patchIndex Row-major patch index.
 * @param x Patch-local X coordinate in samples.
 * @param z Patch-local Z coordinate in samples.
 * @param selection Borrowed fractional LOD and reconciled edge values.
 * @returns World-space height on the continuously morphed triangle surface.
 */
export function sampleMorphedHeight(
  asset: TerrainAsset,
  patchIndex: number,
  x: number,
  z: number,
  selection: TerrainSelection): number {
  const cells = asset.manifest.tileCells;
  const patch = asset.patches[patchIndex];
  const edgeDetail = getEdgeDetail(x, z, cells, selection);

  if((x === 0) || (x === cells) || (z === 0) || (z === cells)) {
    return sampleDetailHeight(asset, patch, cells, x, z, edgeDetail);
  }

  const level = Math.floor(selection.detail);
  const fineStride = 2**level;
  const coarseStride = Math.min(cells, fineStride*2);
  const fineHeight = sampleMorphedSurface(asset, patch, cells, x, z, fineStride, selection);
  const coarseHeight = sampleMorphedSurface(asset, patch, cells, x, z, coarseStride, selection);
  const morph = selection.detail - level;

  return fineHeight + (coarseHeight - fineHeight)*morph;
}

/** Calculate one patch's fractional detail from its screen-space error budget. */
function calculatePatchDetail(
  asset: TerrainAsset,
  patch: TerrainPatch,
  view: TerrainView,
  sampleSpacing: number): number {
  const {size, tileCells, extent} = asset.manifest;
  const minimum = [
    patch.x/(size - 1)*extent[0] - extent[0]/2,
    patch.min,
    patch.z/(size - 1)*extent[1] - extent[1]/2
  ];
  const maximum = [
    minimum[0] + tileCells/(size - 1)*extent[0],
    patch.max,
    minimum[2] + tileCells/(size - 1)*extent[1]
  ];
  const targetDistanceX = Math.max(minimum[0] - view.target[0], 0, view.target[0] - maximum[0]);
  const targetDistanceZ = Math.max(minimum[2] - view.target[2], 0, view.target[2] - maximum[2]);
  const targetDistance = Math.hypot(targetDistanceX, targetDistanceZ);

  if((view.targetRadius > 0) && (targetDistance < view.targetRadius)) {
    return 0;
  }

  const cameraOffsets = minimum.map((value, axis) => {
    return Math.max(value - view.eye[axis], 0, view.eye[axis] - maximum[axis]);
  });
  const cameraDistance = Math.max(MINIMUM_DISTANCE, Math.hypot(...cameraOffsets));
  const errorBudget = cameraDistance/view.projectionScale*view.tolerance;
  const errors = patch.errors.map((error, level) => {
    const cellError = level === 0
      ? 0
      : 2**level*sampleSpacing*view.tolerance/MAXIMUM_CELL_PIXELS;
    return Math.max(error, cellError);
  });

  let level = 0;
  while((level < asset.maxLod) && (errors[level + 1] <= errorBudget)) {
    level+= 1;
  }

  const detail = calculateFractionalDetail(level, asset.maxLod, errorBudget, errors);
  const targetFade = view.targetRadius > 0
    ? clampUnit((targetDistance - view.targetRadius)/view.targetRadius)
    : 1;
  const smoothTargetFade = targetFade*targetFade*(3 - 2*targetFade);

  return detail*smoothTargetFade;
}

/** Calculate continuous detail between a selected lattice and its parent. */
function calculateFractionalDetail(
  level: number,
  maxLod: number,
  errorBudget: number,
  errors: number[]): number {
  if(level === maxLod) {
    return level;
  }

  const errorRange = errors[level + 1] - errors[level];
  const morph = clampUnit((errorBudget - errors[level])/errorRange);
  return level + morph;
}

/** Resolve boundary detail from the reconciled neighbor edges. */
function getEdgeDetail(
  x: number,
  z: number,
  cells: number,
  selection: TerrainSelection): number {
  let detail = selection.detail;

  if(x === 0) {
    detail = selection.edges[0];
  }
  if(x === cells) {
    detail = selection.edges[1];
  }
  if(z === 0) {
    detail = Math.max(detail, selection.edges[2]);
  }
  if(z === cells) {
    detail = Math.max(detail, selection.edges[3]);
  }

  return detail;
}

/** Blend the two authored triangle lattices surrounding a fractional detail. */
function sampleDetailHeight(
  asset: TerrainAsset,
  patch: TerrainPatch,
  cells: number,
  x: number,
  z: number,
  detail: number): number {
  const level = Math.floor(detail);
  const fineStride = 2**level;
  const coarseStride = Math.min(cells, fineStride*2);
  const fineHeight = sampleHeightTriangle(asset, patch.x + x, patch.z + z, fineStride);
  const coarseHeight = sampleHeightTriangle(asset, patch.x + x, patch.z + z, coarseStride);

  return fineHeight + (coarseHeight - fineHeight)*(detail - level);
}

/** Reconstruct the stitched triangle containing one patch-local coordinate. */
function sampleMorphedSurface(
  asset: TerrainAsset,
  patch: TerrainPatch,
  cells: number,
  x: number,
  z: number,
  stride: number,
  selection: TerrainSelection): number {
  const baseX = Math.min(Math.floor(x/stride)*stride, cells - stride);
  const baseZ = Math.min(Math.floor(z/stride)*stride, cells - stride);
  const fractionX = (x - baseX)/stride;
  const fractionZ = (z - baseZ)/stride;
  const topLeft = sampleStitchedVertex(asset, patch, cells, baseX, baseZ, stride, selection);
  const topRight = sampleStitchedVertex(asset, patch, cells, baseX + stride, baseZ, stride, selection);
  const bottomLeft = sampleStitchedVertex(asset, patch, cells, baseX, baseZ + stride, stride, selection);
  const bottomRight = sampleStitchedVertex(asset, patch, cells, baseX + stride, baseZ + stride, stride, selection);

  return fractionX >= fractionZ
    ? topLeft + (topRight - topLeft)*fractionX + (bottomRight - topRight)*fractionZ
    : topLeft + (bottomRight - bottomLeft)*fractionX + (bottomLeft - topLeft)*fractionZ;
}

/** Sample a parent vertex after applying any coarser neighboring edge. */
function sampleStitchedVertex(
  asset: TerrainAsset,
  patch: TerrainPatch,
  cells: number,
  x: number,
  z: number,
  stride: number,
  selection: TerrainSelection): number {
  const ownDetail = Math.log2(stride);
  const edgeDetail = getEdgeDetail(x, z, cells, {
    ...selection,
    detail: ownDetail
  });

  return sampleDetailHeight(asset, patch, cells, x, z, edgeDetail);
}

/** Determine whether a borrowed value is a complete finite three-component vector. */
function isFiniteVector3(value: number[]): boolean {
  return Array.isArray(value)
    && (value.length === VECTOR_COMPONENT_COUNT)
    && Array.from(value).every(Number.isFinite);
}

/** Clamp a scalar to the closed normalized interval. */
function clampUnit(value: number): number {
  return Math.max(0, Math.min(1, value));
}
