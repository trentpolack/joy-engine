// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

export interface TerrainPatch {
  x: number;
  z: number;
  min: number;
  max: number;
  errors: number[];
}
export interface TerrainManifest {
  version: number;
  size: number;
  tileCells: number;
  extent: number[];
  heightOffset: number;
  heightScale: number;
  heightFile: string;
  orientation: string;
  patches: TerrainPatch[];
}
export interface TerrainAsset {
  manifest: TerrainManifest;
  heights: Uint16Array;
  patches: TerrainPatch[];
  tilesPerAxis: number;
  maxLod: number;
}

/**
 * Load a caller-owned resident heightfield. Binary is uint16 little endian,
 * row-major; source row zero maps to negative world Z, columns to positive X.
 * Scales are explicit world units; camera LOD never changes this data.
 * @param url
 */
export async function loadTerrain(url: string | URL): Promise<TerrainAsset> {
  const manifestUrl = new URL(url, globalThis.location?.href);
  const response = await fetch(manifestUrl);
  if(!response.ok) {
    throw new Error(`Terrain manifest: ${response.status}`);
  }
  const manifest = await response.json();
  const heightResponse = await fetch(new URL(manifest.heightFile, manifestUrl));
  if(!heightResponse.ok) {
    throw new Error(`Terrain heights: ${heightResponse.status}`);
  }
  return decodeTerrain(manifest, await heightResponse.arrayBuffer());
}

/**
 * Validate before GPU allocation and decode portable little-endian samples.
 * Copies binary input; consumers must treat returned data as immutable.
 * @param manifest @param binary
 */
export function decodeTerrain(manifest: TerrainManifest, binary: ArrayBuffer): TerrainAsset {
  const {size, tileCells, extent, heightOffset, heightScale, patches} = manifest;
  if(manifest.version !== 1 || manifest.orientation !== 'row-0-to-negative-z'
    || !Number.isInteger(size) || size < 3 || size > 4097 || ((size - 1) & (size - 2)) !== 0
    || !Number.isInteger(tileCells) || tileCells < 2 || tileCells > 128
    || (tileCells & (tileCells - 1)) !== 0 || (size - 1) % tileCells !== 0
    || !Array.isArray(extent) || extent.length !== 2 || ![extent[0],extent[1]].every(v => Number.isFinite(v) && v > 0)
    || !Number.isFinite(heightOffset) || !Number.isFinite(heightScale) || heightScale < 0
    || binary.byteLength !== size*size*2) {
    throw new Error('Invalid terrain dimensions, orientation, scales or height payload.');
  }
  // Power-of-two cell counts make every coarser lattice land on authored
  // samples and preserve patch corners all the way to the coarsest level.
  const tilesPerAxis = (size - 1)/tileCells;
  const maxLod = Math.log2(tileCells);
  if(!Array.isArray(patches) || patches.length !== tilesPerAxis**2 || patches.length > 4096) {
    throw new Error('Invalid terrain patch count.');
  }
  // Selection and neighbor lookup rely on a complete row-major leaf grid.
  // Monotone errors let selection stop at the first level over its budget.
  patches.forEach((patch, index) => {
    if(patch.x !== (index % tilesPerAxis)*tileCells || patch.z !== Math.floor(index/tilesPerAxis)*tileCells
      || !Number.isFinite(patch.min) || !Number.isFinite(patch.max) || patch.min > patch.max
      || !Array.isArray(patch.errors) || patch.errors.length !== maxLod + 1 || patch.errors[0] !== 0
      || !Array.from(patch.errors).every((v, i) => Number.isFinite(v) && v >= 0 && (i === 0 || v >= patch.errors[i - 1]))) {
      throw new Error('Invalid terrain patch bounds or monotone error hierarchy.');
    }
  });

  // Read byte order explicitly instead of depending on the host's typed-array
  // endianness. Keep an owned copy so callers can release the binary payload.
  const view = new DataView(binary);
  const heights = new Uint16Array(size*size);
  for(let i = 0; i < heights.length; i++) {
    heights[i] = view.getUint16(i*2, true);
  }
  return {manifest,heights,patches,tilesPerAxis,maxLod};
}

/**
 * Stable collision/gameplay sample, independent of camera and render topology.
 * Clamps to landscape bounds. Uses the same diagonal triangle surface at stride 1.
 * @param asset @param worldX @param worldZ
 */
export function sampleTerrain(asset: TerrainAsset, worldX: number, worldZ: number) {
  const {size,extent} = asset.manifest;
  // World X/Z are centered on the landscape; sample coordinates start at zero.
  // Always sample the finest resident surface, regardless of the camera LOD.
  return sampleHeightTriangle(asset,(worldX/extent[0] + 0.5)*(size - 1),(worldZ/extent[1] + 0.5)*(size - 1),1);
}

/**
 * Sample authored triangle lattice at an integer power-of-two stride.
 * @param asset @param x @param z @param stride
 */
export function sampleHeightTriangle(asset: TerrainAsset, x: number, z: number, stride: number) {
  const {size,heightOffset,heightScale} = asset.manifest;
  x = Math.max(0,Math.min(size - 1,x));
  z = Math.max(0,Math.min(size - 1,z));

  // At the positive boundary, use the final complete cell with fraction 1
  // rather than a cell beginning outside the heightfield.
  const bx = Math.min(size - 1 - stride,Math.floor(x/stride)*stride);
  const bz = Math.min(size - 1 - stride,Math.floor(z/stride)*stride);
  const fx = (x - bx)/stride, fz = (z - bz)/stride;
  const a = asset.heights[bz*size + bx], b = asset.heights[bz*size + bx + stride];
  const c = asset.heights[(bz + stride)*size + bx], d = asset.heights[(bz + stride)*size + bx + stride];

  // The a-to-d diagonal divides the cell into two planes. Bilinear filtering
  // would produce a different surface from the triangles used for rendering.
  const quantized = fx >= fz ? a + (b - a)*fx + (d - b)*fz : a + (d - c)*fx + (c - a)*fz;
  return heightOffset + quantized/65535*heightScale;
}
