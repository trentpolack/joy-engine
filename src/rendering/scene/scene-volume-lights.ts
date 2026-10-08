// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { GPU_BACKEND } from '../gpu/constants.ts';
import { Buffer } from '@luma.gl/core';
import type { Device } from '@luma.gl/core';
import type { PointLight } from '../pbr/point-lights.ts';

export interface SceneVolumeLightOptions {
  /** Stable input-order retention. Hard ceiling: 32 point lights. */
  maxPointLights?: number;
  /** Stable input-order retention in each cluster. Hard ceiling: eight candidates. */
  maxLightsPerCluster?: number;
  /** Backing-pixel tile size; dimensions are capped independently of viewport growth. */
  tileSize?: number;
  maxClusterCountX?: number;
  maxClusterCountY?: number;
  clusterCountZ?: number;
}

export interface SceneVolumeLightCamera {
  /** Borrowed column-major rigid world-to-view transform; forward is negative Z. */
  viewMatrix: ArrayLike<number>;
  /** Borrowed column-major perspective projection, matching the rendered frame. */
  projectionMatrix: ArrayLike<number>;
  /** Positive view-space distances in world units. */
  nearPlane: number;
  farPlane: number;
  /** Positive backing-pixel viewport dimensions. */
  width: number;
  height: number;
}

/** Borrowed storage bindings valid until this owner's next update or destruction. */
export interface SceneVolumeLightBindings {
  pointLights: Buffer;
  clusterLightCounts: Buffer;
  clusterLightIndices: Buffer;
  clusterCountX: number;
  clusterCountY: number;
  clusterCountZ: number;
  maxLightsPerCluster: number;
  pointLightCount: number;
}

export interface SceneVolumeLightStats {
  sourceLightCount: number;
  pointLightCount: number;
  /** Input lights omitted by the global storage limit. */
  droppedLightCount: number;
  /** Candidate memberships omitted by per-cluster limits; one light may count repeatedly. */
  droppedClusterLightCount: number;
  clusterCount: number;
}

const MAX_VOLUME_POINT_LIGHTS = 32;
const MAX_VOLUME_CLUSTER_LIGHTS = 8;
const MAX_CLUSTER_COUNT_X = 16;
const MAX_CLUSTER_COUNT_Y = 9;
const MAX_CLUSTER_COUNT_Z = 16;
const DEFAULT_TILE_SIZE = 128;
const LIGHT_FLOAT_COUNT = 8;
const POINT_LIGHT_BUFFER_ID = 'scene-volume-point-lights';
const CLUSTER_COUNT_BUFFER_ID = 'scene-volume-cluster-counts';
const CLUSTER_INDEX_BUFFER_ID = 'scene-volume-cluster-indices';

/**
 * Owns bounded WebGPU storage for luma.gl clustered participating-media integration.
 * Borrows the device and frame lights. Rebuilds view-space light positions and conservative
 * sphere/frustum memberships every frame, including stationary lights under a moving camera.
 * Defaults cap CPU work at 32 lights and 16×9×16 clusters with eight candidates each.
 * Buffers grow on demand and retain capacity on shrink; destroy before destroying the device.
 */
export class SceneVolumeLights {
  private readonly device: Device;
  private readonly options: Required<SceneVolumeLightOptions>;
  private readonly lightData: Float32Array;
  private pointLights: Buffer | null = null;
  private clusterLightCounts: Buffer | null = null;
  private clusterLightIndices: Buffer | null = null;
  private clusterCapacity = 0;
  private countData: Uint32Array = new Uint32Array(0);
  private indexData: Uint32Array = new Uint32Array(0);
  private destroyed = false;
  stats: Readonly<SceneVolumeLightStats>;

  constructor(device: Device, options: SceneVolumeLightOptions = {}) {
    if(device.type !== GPU_BACKEND.WEBGPU) {
      throw new Error('Scene volume-light storage requires a WebGPU device.');
    }
    this.device = device;
    this.options = {
      maxPointLights: options.maxPointLights ?? MAX_VOLUME_POINT_LIGHTS,
      maxLightsPerCluster: options.maxLightsPerCluster ?? MAX_VOLUME_CLUSTER_LIGHTS,
      tileSize: options.tileSize ?? DEFAULT_TILE_SIZE,
      maxClusterCountX: options.maxClusterCountX ?? MAX_CLUSTER_COUNT_X,
      maxClusterCountY: options.maxClusterCountY ?? MAX_CLUSTER_COUNT_Y,
      clusterCountZ: options.clusterCountZ ?? MAX_CLUSTER_COUNT_Z
    };
    validateBound(this.options.maxPointLights, MAX_VOLUME_POINT_LIGHTS);
    validateBound(this.options.maxLightsPerCluster, MAX_VOLUME_CLUSTER_LIGHTS);
    validateBound(this.options.maxClusterCountX, MAX_CLUSTER_COUNT_X);
    validateBound(this.options.maxClusterCountY, MAX_CLUSTER_COUNT_Y);
    validateBound(this.options.clusterCountZ, MAX_CLUSTER_COUNT_Z);
    validateBound(this.options.tileSize, Number.MAX_SAFE_INTEGER);
    this.lightData = new Float32Array(this.options.maxPointLights*LIGHT_FLOAT_COUNT);
    this.stats = {sourceLightCount: 0, pointLightCount: 0, droppedLightCount: 0,
      droppedClusterLightCount: 0, clusterCount: 0};
  }

  /**
   * Upload a borrowed frame; does not mutate lights or camera matrices.
   * RGB is nonnegative linear irradiance; range is a positive world-space radius.
   * Returns actual luma.gl storage bindings, with no ownership transfer to the caller.
   * Invalid input throws before allocating or writing GPU resources.
   */
  update(lights: readonly PointLight[], camera: SceneVolumeLightCamera): SceneVolumeLightBindings {
    if(this.destroyed) {
      throw new Error('Scene volume lights are destroyed.');
    }
    validateCamera(camera);
    const pointLightCount = Math.min(lights.length, this.options.maxPointLights);
    this.packLights(lights, camera.viewMatrix, pointLightCount);

    const clusterCountX = Math.min(this.options.maxClusterCountX, Math.ceil(camera.width/this.options.tileSize));
    const clusterCountY = Math.min(this.options.maxClusterCountY, Math.ceil(camera.height/this.options.tileSize));
    const clusterCountZ = this.options.clusterCountZ;
    const clusterCount = clusterCountX*clusterCountY*clusterCountZ;
    const horizontalPlanes = createTilePlanes(camera.projectionMatrix, clusterCountX, 0);
    const verticalPlanes = createTilePlanes(camera.projectionMatrix, clusterCountY, 1);
    this.ensureStorage(clusterCount);
    const counts = this.countData.subarray(0, clusterCount);
    const indices = this.indexData.subarray(0, clusterCount*this.options.maxLightsPerCluster);
    counts.fill(0);
    indices.fill(0);

    const droppedClusterLightCount = this.clusterLights(camera, pointLightCount,
      clusterCountX, clusterCountY, clusterCountZ, horizontalPlanes, verticalPlanes);
    // The fixed minimum point allocation remains bindable even for an empty light list.
    const pointLights = this.pointLights!;
    const clusterLightCounts = this.clusterLightCounts!;
    const clusterLightIndices = this.clusterLightIndices!;
    pointLights.write(this.lightData);
    clusterLightCounts.write(counts);
    clusterLightIndices.write(indices);
    this.stats = {sourceLightCount: lights.length, pointLightCount,
      droppedLightCount: lights.length - pointLightCount, droppedClusterLightCount, clusterCount};
    return {pointLights, clusterLightCounts, clusterLightIndices, clusterCountX, clusterCountY,
      clusterCountZ, maxLightsPerCluster: this.options.maxLightsPerCluster, pointLightCount};
  }

  /** Idempotently release all owned storage. The borrowed device remains alive. */
  destroy() {
    if(this.destroyed) {
      return;
    }
    this.destroyed = true;
    this.pointLights?.destroy();
    this.clusterLightCounts?.destroy();
    this.clusterLightIndices?.destroy();
    this.pointLights = null;
    this.clusterLightCounts = null;
    this.clusterLightIndices = null;
    this.countData = new Uint32Array(0);
    this.indexData = new Uint32Array(0);
  }

  private packLights(lights: readonly PointLight[], view: ArrayLike<number>, count: number) {
    this.lightData.fill(0);
    for(let index = 0; index < count; index++) {
      const light = lights[index];
      const values = [light.x, light.y, light.z, light.range, light.r, light.g, light.b];
      if(!values.every(Number.isFinite) || light.range <= 0 || Math.min(light.r, light.g, light.b) < 0) {
        throw new RangeError('Volume light position/color must be finite and range positive.');
      }
      const offset = index*LIGHT_FLOAT_COUNT;
      // Upstream ABI: positionRange vec4f followed by colorIntensity vec4f (32 bytes).
      this.lightData.set([
        view[0]*light.x + view[4]*light.y + view[8]*light.z + view[12],
        view[1]*light.x + view[5]*light.y + view[9]*light.z + view[13],
        view[2]*light.x + view[6]*light.y + view[10]*light.z + view[14],
        light.range, light.r, light.g, light.b, 1
      ], offset);
      if(!this.lightData.subarray(offset, offset + LIGHT_FLOAT_COUNT).every(Number.isFinite)
        || this.lightData[offset + 3] <= 0) {
        throw new RangeError('Volume light values must fit finite float32 values and preserve positive range.');
      }
    }
  }

  private ensureStorage(clusterCount: number) {
    if(this.pointLights && clusterCount <= this.clusterCapacity) {
      return;
    }
    let capacity = Math.max(1, this.clusterCapacity);
    while(capacity < clusterCount) {
      capacity*= 2;
    }
    capacity = Math.min(capacity, this.options.maxClusterCountX*this.options.maxClusterCountY*this.options.clusterCountZ);
    const usage = Buffer.STORAGE | Buffer.COPY_DST;
    const countBytes = capacity*Uint32Array.BYTES_PER_ELEMENT;
    const indexBytes = countBytes*this.options.maxLightsPerCluster;
    const pointBytes = this.lightData.byteLength;
    const maximumBytes = Math.min(this.device.limits.maxBufferSize, this.device.limits.maxStorageBufferBindingSize);
    if(Math.max(countBytes, indexBytes, pointBytes) > maximumBytes) {
      throw new RangeError('Volume light storage exceeds the device buffer binding limits.');
    }

    // Allocate a complete replacement before touching active bindings. A partial failure
    // releases only new resources and leaves the previous frame's buffers usable.
    let pointLights: Buffer | null = null;
    let counts: Buffer | null = null;
    let indices: Buffer | null = null;
    let countData: Uint32Array;
    let indexData: Uint32Array;
    try {
      countData = new Uint32Array(capacity);
      indexData = new Uint32Array(capacity*this.options.maxLightsPerCluster);
      if(!this.pointLights) {
        pointLights = this.device.createBuffer({id: POINT_LIGHT_BUFFER_ID, usage, byteLength: pointBytes});
      }
      counts = this.device.createBuffer({id: CLUSTER_COUNT_BUFFER_ID, usage, byteLength: countBytes});
      indices = this.device.createBuffer({id: CLUSTER_INDEX_BUFFER_ID, usage, byteLength: indexBytes});
    } catch(error) {
      pointLights?.destroy();
      counts?.destroy();
      indices?.destroy();
      throw error;
    }
    this.clusterLightCounts?.destroy();
    this.clusterLightIndices?.destroy();
    this.pointLights ??= pointLights;
    this.clusterLightCounts = counts;
    this.clusterLightIndices = indices;
    this.countData = countData;
    this.indexData = indexData;
    this.clusterCapacity = capacity;
  }

  private clusterLights(camera: SceneVolumeLightCamera, lightCount: number, countX: number, countY: number,
    countZ: number, horizontalPlanes: Float64Array, verticalPlanes: Float64Array) {
    const depthRatio = camera.farPlane/camera.nearPlane;
    let dropped = 0;
    // Ascending light order gives deterministic overflow and avoids per-cluster sorting.
    for(let lightIndex = 0; lightIndex < lightCount; lightIndex++) {
      const offset = lightIndex*LIGHT_FLOAT_COUNT;
      const x = this.lightData[offset];
      const y = this.lightData[offset + 1];
      const z = this.lightData[offset + 2];
      const radius = this.lightData[offset + 3];
      for(let tileZ = 0; tileZ < countZ; tileZ++) {
        const near = camera.nearPlane*depthRatio**(tileZ/countZ);
        const far = camera.nearPlane*depthRatio**((tileZ + 1)/countZ);
        if(-z + radius < near || -z - radius > far) {
          continue;
        }
        for(let tileY = 0; tileY < countY; tileY++) {
          if(!intersectsTile(verticalPlanes, tileY, x, y, z, radius)) {
            continue;
          }
          for(let tileX = 0; tileX < countX; tileX++) {
            if(!intersectsTile(horizontalPlanes, tileX, x, y, z, radius)) {
              continue;
            }
            // Upstream uses top-left texture coordinates and x-fastest linear indexing.
            const cluster = tileX + countX*(tileY + countY*tileZ);
            const candidateCount = this.countData[cluster];
            if(candidateCount >= this.options.maxLightsPerCluster) {
              dropped++;
              continue;
            }
            this.indexData[cluster*this.options.maxLightsPerCluster + candidateCount] = lightIndex;
            this.countData[cluster] = candidateCount + 1;
          }
        }
      }
    }
    return dropped;
  }
}

function validateBound(value: number, maximum: number) {
  if(!Number.isInteger(value) || value < 1 || value > maximum) {
    throw new RangeError(`Volume light capacity must be an integer from 1 through ${maximum}.`);
  }
}

function validateCamera(camera: SceneVolumeLightCamera) {
  for(const matrix of [camera.viewMatrix, camera.projectionMatrix]) {
    if(matrix.length !== 16 || !Array.from(matrix).every(value => Number.isFinite(value) && Number.isFinite(Math.fround(value)))) {
      throw new RangeError('Volume light camera requires finite float32 4×4 matrices.');
    }
  }
  if(![camera.width, camera.height, camera.nearPlane, camera.farPlane].every(Number.isFinite)
    || camera.width <= 0 || camera.height <= 0 || camera.nearPlane <= 0
    || camera.farPlane <= camera.nearPlane || !Number.isFinite(camera.farPlane/camera.nearPlane)) {
    throw new RangeError('Volume light camera requires a positive viewport and ordered positive clipping planes.');
  }
}

/** Normalize inward-facing clip planes so sphere radius can be compared in view units. */
function createTilePlanes(projection: ArrayLike<number>, count: number, axis: number): Float64Array {
  const planes = new Float64Array(count*8);
  for(let tile = 0; tile < count; tile++) {
    const minimum = axis === 0 ? 2*tile/count - 1 : 1 - 2*(tile + 1)/count;
    const maximum = axis === 0 ? 2*(tile + 1)/count - 1 : 1 - 2*tile/count;
    for(let side = 0; side < 2; side++) {
      const sign = side === 0 ? 1 : -1;
      const boundary = side === 0 ? minimum : maximum;
      const offset = tile*8 + side*4;
      for(let component = 0; component < 4; component++) {
        planes[offset + component] = sign*(projection[component*4 + axis] - boundary*projection[component*4 + 3]);
      }
      const length = Math.hypot(planes[offset], planes[offset + 1], planes[offset + 2]);
      if(!Number.isFinite(length) || length <= 0) {
        throw new RangeError('Volume light projection must define finite nondegenerate cluster planes.');
      }
      for(let component = 0; component < 4; component++) {
        planes[offset + component]/= length;
      }
    }
  }
  return planes;
}

function intersectsTile(planes: Float64Array, tile: number, x: number, y: number, z: number, radius: number) {
  for(let side = 0; side < 2; side++) {
    const offset = tile*8 + side*4;
    const distance = planes[offset]*x + planes[offset + 1]*y + planes[offset + 2]*z + planes[offset + 3];
    if(distance < -radius) {
      return false;
    }
  }
  return true;
}
