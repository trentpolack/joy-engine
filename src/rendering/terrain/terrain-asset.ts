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

const TERRAIN_FORMAT_VERSION = 1;
const TERRAIN_ORIENTATION = 'row-0-to-negative-z';
const MINIMUM_HEIGHTFIELD_SIZE = 3;
const MAXIMUM_HEIGHTFIELD_SIZE = 4097;
const MINIMUM_TILE_CELLS = 2;
const MAXIMUM_TILE_CELLS = 128;
const MAXIMUM_PATCH_COUNT = 4096;
const BYTES_PER_HEIGHT_SAMPLE = 2;
const MAXIMUM_QUANTIZED_HEIGHT = 65535;

/**
 * Load a caller-owned resident heightfield. Binary is uint16 little endian,
 * row-major; source row zero maps to negative world Z, columns to positive X.
 * Scales are explicit world units; camera LOD never changes this data.
 * @param url URL of the terrain manifest, resolved against the current page when relative.
 * @returns A caller-owned terrain asset containing copied height samples.
 * @throws When either request fails or the downloaded asset is invalid.
 */
export async function loadTerrain(url: string | URL): Promise<TerrainAsset> {
  const manifestUrl = new URL(url, globalThis.location?.href);
  const response = await fetch(manifestUrl);
  if(!response.ok) {
    throw new Error(`Terrain manifest: ${response.status}`);
  }
  const manifest = await response.json() as TerrainManifest;
  const heightResponse = await fetch(new URL(manifest.heightFile, manifestUrl));
  if(!heightResponse.ok) {
    throw new Error(`Terrain heights: ${heightResponse.status}`);
  }
  return decodeTerrain(manifest, await heightResponse.arrayBuffer());
}

/**
 * Validate before GPU allocation and decode portable little-endian samples.
 * Copies binary input; consumers must treat returned data as immutable.
 * @param manifest Borrowed manifest describing the heightfield and patch hierarchy.
 * @param binary Borrowed little-endian height payload; it is not retained or mutated.
 * @returns A caller-owned terrain asset with copied height samples.
 * @throws When dimensions, metadata, patches, or binary length are invalid.
 */
export function decodeTerrain(manifest: TerrainManifest, binary: ArrayBuffer): TerrainAsset {
  const {size, tileCells, patches} = manifest;

  if(!hasValidTerrainDimensions(manifest, binary.byteLength)) {
    throw new Error('Invalid terrain dimensions, orientation, scales or height payload.');
  }

  // Power-of-two cell counts make every coarser lattice land on authored
  // samples and preserve patch corners all the way to the coarsest level.
  const tilesPerAxis = (size - 1)/tileCells;
  const maxLod = Math.log2(tileCells);
  if(!Array.isArray(patches)
    || (patches.length !== tilesPerAxis**2)
    || (patches.length > MAXIMUM_PATCH_COUNT)) {
    throw new Error('Invalid terrain patch count.');
  }

  // Selection and neighbor lookup rely on a complete row-major leaf grid.
  // Monotone errors let selection stop at the first level over its budget.
  patches.forEach((patch, patchIndex) => {
    if(!isValidTerrainPatch(patch, patchIndex, tilesPerAxis, tileCells, maxLod)) {
      throw new Error('Invalid terrain patch bounds or monotone error hierarchy.');
    }
  });

  // Read byte order explicitly instead of depending on the host's typed-array
  // endianness. Keep an owned copy so callers can release the binary payload.
  const view = new DataView(binary);
  const heights = new Uint16Array(size*size);
  for(let i = 0; i < heights.length; i++) {
    heights[i] = view.getUint16(i*BYTES_PER_HEIGHT_SAMPLE, true);
  }

  return {
    manifest,
    heights,
    patches,
    tilesPerAxis,
    maxLod
  };
}

/**
 * Stable collision/gameplay sample, independent of camera and render topology.
 * Clamps to landscape bounds. Uses the same diagonal triangle surface at stride 1.
 * @param asset Borrowed resident terrain data.
 * @param worldX World-space X coordinate.
 * @param worldZ World-space Z coordinate.
 * @returns Interpolated world-space height at the clamped coordinate.
 */
export function sampleTerrain(asset: TerrainAsset, worldX: number, worldZ: number) {
  const {size, extent} = asset.manifest;
  // World X/Z are centered on the landscape; sample coordinates start at zero.
  // Always sample the finest resident surface, regardless of the camera LOD.
  const sampleX = (worldX/extent[0] + 0.5)*(size - 1);
  const sampleZ = (worldZ/extent[1] + 0.5)*(size - 1);

  return sampleHeightTriangle(asset, sampleX, sampleZ, 1);
}

/**
 * Sample authored triangle lattice at an integer power-of-two stride.
 * @param asset Borrowed resident terrain data.
 * @param x Heightfield-space X coordinate in samples.
 * @param z Heightfield-space Z coordinate in samples.
 * @param stride Positive power-of-two sample interval.
 * @returns Interpolated world-space height on the rendered triangle surface.
 */
export function sampleHeightTriangle(asset: TerrainAsset, x: number, z: number, stride: number) {
  const {size, heightOffset, heightScale} = asset.manifest;
  const clampedX = Math.max(0, Math.min(size - 1, x));
  const clampedZ = Math.max(0, Math.min(size - 1, z));

  // At the positive boundary, use the final complete cell with fraction 1
  // rather than a cell beginning outside the heightfield.
  const baseX = Math.min(size - 1 - stride, Math.floor(clampedX/stride)*stride);
  const baseZ = Math.min(size - 1 - stride, Math.floor(clampedZ/stride)*stride);
  const fractionX = (clampedX - baseX)/stride;
  const fractionZ = (clampedZ - baseZ)/stride;
  const topLeft = asset.heights[baseZ*size + baseX];
  const topRight = asset.heights[baseZ*size + baseX + stride];
  const bottomLeft = asset.heights[(baseZ + stride)*size + baseX];
  const bottomRight = asset.heights[(baseZ + stride)*size + baseX + stride];

  // The a-to-d diagonal divides the cell into two planes. Bilinear filtering
  // would produce a different surface from the triangles used for rendering.
  const quantizedHeight = fractionX >= fractionZ
    ? topLeft + (topRight - topLeft)*fractionX + (bottomRight - topRight)*fractionZ
    : topLeft + (bottomRight - bottomLeft)*fractionX + (bottomLeft - topLeft)*fractionZ;

  return heightOffset + quantizedHeight/MAXIMUM_QUANTIZED_HEIGHT*heightScale;
}

/** Validate manifest-wide dimensions and scalar terrain metadata. */
function hasValidTerrainDimensions(manifest: TerrainManifest, binaryByteLength: number): boolean {
  const {size, tileCells, extent, heightOffset, heightScale} = manifest;
  const heightfieldCells = size - 1;
  const hasPowerOfTwoSize = (heightfieldCells & (heightfieldCells - 1)) === 0;
  const hasPowerOfTwoTileSize = (tileCells & (tileCells - 1)) === 0;
  const hasValidExtent = Array.isArray(extent)
    && (extent.length === 2)
    && extent.every(value => Number.isFinite(value) && (value > 0));

  return (manifest.version === TERRAIN_FORMAT_VERSION)
    && (manifest.orientation === TERRAIN_ORIENTATION)
    && Number.isInteger(size)
    && (size >= MINIMUM_HEIGHTFIELD_SIZE)
    && (size <= MAXIMUM_HEIGHTFIELD_SIZE)
    && hasPowerOfTwoSize
    && Number.isInteger(tileCells)
    && (tileCells >= MINIMUM_TILE_CELLS)
    && (tileCells <= MAXIMUM_TILE_CELLS)
    && hasPowerOfTwoTileSize
    && ((heightfieldCells % tileCells) === 0)
    && hasValidExtent
    && Number.isFinite(heightOffset)
    && Number.isFinite(heightScale)
    && (heightScale >= 0)
    && (binaryByteLength === size*size*BYTES_PER_HEIGHT_SAMPLE);
}

/** Validate one patch's row-major position, bounds, and monotone error hierarchy. */
function isValidTerrainPatch(
  patch: TerrainPatch,
  patchIndex: number,
  tilesPerAxis: number,
  tileCells: number,
  maxLod: number): boolean {
  const expectedX = (patchIndex % tilesPerAxis)*tileCells;
  const expectedZ = Math.floor(patchIndex/tilesPerAxis)*tileCells;
  const hasValidBounds = Number.isFinite(patch.min)
    && Number.isFinite(patch.max)
    && (patch.min <= patch.max);
  const hasValidErrors = Array.isArray(patch.errors)
    && (patch.errors.length === maxLod + 1)
    && (patch.errors[0] === 0)
    && patch.errors.every((error, level) => {
      const previousError = level > 0 ? patch.errors[level - 1] : 0;
      return Number.isFinite(error) && (error >= 0) && (error >= previousError);
    });

  return (patch.x === expectedX)
    && (patch.z === expectedZ)
    && hasValidBounds
    && hasValidErrors;
}
