// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {RenderScene} from './render-scene.ts';
import type {MeshInstance} from './mesh-instance.ts';
import type {PreparedMesh} from './mesh-frame.ts';

import {sortCameraTransparentTriangles} from '../camera/orbit-camera.ts';
import {outsideFrustum} from './mesh-transform.ts';
import {prepareMesh} from './mesh-frame.ts';

// Interleaved XYZ/RGBA floats per vertex.
const FLOATS_PER_VERTEX = 7;
const DEFAULT_SURFACE_ROUGHNESS = 1;
const MINIMUM_SURFACE_ROUGHNESS = 0;
const MAXIMUM_SURFACE_ROUGHNESS = 1;

interface SceneFrameOptions {
  backward: readonly number[];
  viewProjection?: ArrayLike<number>;
  webgpu?: boolean;
  opaqueTriangles?: readonly number[];
  transparentTriangles?: readonly number[];
}

/** One instance's cached geometry and destination, decided before any copying. */
interface RoutedMesh {
  instance: MeshInstance;
  prepared: PreparedMesh;
  translucent: boolean;
  culled: boolean;
}

/** Exact queue sizes and visibility totals collected during routing. */
interface SceneQueuePlan {
  routedMeshes: RoutedMesh[];
  opaqueLength: number;
  casterLength: number;
  transparentLength: number;
  visibleCount: number;
  culledCount: number;
}

/**
 * Prepare owned vertex-color queues without mutating the scene, assets, or borrowed triangles.
 * Authored normals, UVs, and materials stay on MeshAsset for other adapters; this adapter matches
 * the existing flat-lit colored-triangle games. Transparency uses global centroid sorting.
 *
 * Queues are assembled in two stages: route and size every instance, then copy each cached
 * Float32Array once with `TypedArray.set`. Order matches submission order: borrowed triangles
 * first, then instances in scene order. The returned queues, ranges, roughness values, and counts
 * are owned by the caller. Invalid visible opaque roughness throws before any queue is returned.
 * @param scene Borrowed live scene whose instance caches may be prepared or reused.
 * @param options Camera state and borrowed transient triangles for this rendered frame.
 * @returns Newly allocated render queues, opaque ranges, and visibility totals.
 */
export function prepareSceneFrame(
  scene: RenderScene,
  {
    backward,
    viewProjection,
    webgpu = false,
    opaqueTriangles = [],
    transparentTriangles = []
  }: SceneFrameOptions) {
  scene.assertAlive();

  const queuePlan = planSceneQueues(
    scene,
    viewProjection,
    webgpu,
    opaqueTriangles.length,
    transparentTriangles.length
  );

  return assembleSceneQueues(queuePlan, backward, opaqueTriangles, transparentTriangles);
}

/**
 * Prepare cached instance geometry and calculate the exact destination queue sizes.
 * @param scene Borrowed live scene containing instances in submission order.
 * @param viewProjection Optional column-major camera matrix used for culling.
 * @param webgpu Whether clip-space culling should use WebGPU depth conventions.
 * @param borrowedOpaqueLength Number of packed borrowed opaque values.
 * @param borrowedTransparentLength Number of packed borrowed transparent values.
 * @returns An owned queue plan that borrows scene instances and their cached geometry.
 */
function planSceneQueues(
  scene: RenderScene,
  viewProjection: ArrayLike<number> | undefined,
  webgpu: boolean,
  borrowedOpaqueLength: number,
  borrowedTransparentLength: number): SceneQueuePlan {
  const routedMeshes: RoutedMesh[] = [];
  let opaqueLength = borrowedOpaqueLength;
  let casterLength = borrowedOpaqueLength;
  let transparentLength = borrowedTransparentLength;
  let visibleCount = 0;
  let culledCount = 0;

  for(const instance of scene.instances) {
    if(!instance.visible || (instance.opacity === 0)) {
      continue;
    }

    const prepared = prepareMesh(instance, scene.frameCache);
    const translucent = isTranslucent(instance);
    if(!translucent) {
      casterLength+= prepared.vertices.length;
    }

    const culled = Boolean(viewProjection && outsideFrustum(instance.asset, prepared.transform, viewProjection, webgpu));
    if(culled) {
      culledCount++;
    } else {
      visibleCount++;

      if(translucent) {
        transparentLength+= prepared.vertices.length;
      } else {
        validateSurfaceRoughness(instance.sceneRoughness);
        opaqueLength+= prepared.vertices.length;
      }
    }

    routedMeshes.push({
      instance,
      prepared,
      translucent,
      culled
    });
  }

  return {
    routedMeshes,
    opaqueLength,
    casterLength,
    transparentLength,
    visibleCount,
    culledCount
  };
}

/**
 * Copy planned geometry into exactly sized, caller-owned frame queues.
 * Camera-culled opaque geometry remains in the shadow-caster queue.
 * @param queuePlan Planned geometry, destinations, and visibility totals.
 * @param backward Camera backward direction used to sort transparent triangles.
 * @param opaqueTriangles Borrowed transient opaque geometry in packed XYZ/RGBA order.
 * @param transparentTriangles Borrowed transient transparent geometry in packed XYZ/RGBA order.
 * @returns Newly allocated render queues, opaque ranges, and visibility totals.
 */
function assembleSceneQueues(
  queuePlan: SceneQueuePlan,
  backward: readonly number[],
  opaqueTriangles: readonly number[],
  transparentTriangles: readonly number[]) {
  const opaque = new Float32Array(queuePlan.opaqueLength);
  const shadowCasters = new Float32Array(queuePlan.casterLength);
  const transparent = new Float32Array(queuePlan.transparentLength);
  const surfaceRoughness: number[] = new Array(queuePlan.opaqueLength/FLOATS_PER_VERTEX)
    .fill(DEFAULT_SURFACE_ROUGHNESS);
  const opaqueRanges = [];

  opaque.set(opaqueTriangles);
  shadowCasters.set(opaqueTriangles);
  transparent.set(transparentTriangles);

  let opaqueOffset = opaqueTriangles.length;
  let casterOffset = opaqueTriangles.length;
  let transparentOffset = transparentTriangles.length;

  for(const routedMesh of queuePlan.routedMeshes) {
    const {instance, prepared, translucent, culled} = routedMesh;
    const vertices = prepared.vertices;

    if(!translucent) {
      shadowCasters.set(vertices, casterOffset);
      casterOffset+= vertices.length;
    }
    if(culled) {
      continue;
    }
    if(translucent) {
      transparent.set(vertices, transparentOffset);
      transparentOffset+= vertices.length;
      continue;
    }

    const firstVertex = opaqueOffset/FLOATS_PER_VERTEX;
    const vertexCount = vertices.length/FLOATS_PER_VERTEX;
    opaqueRanges.push({
      identity: instance,
      topology: instance.asset,
      firstVertex,
      vertexCount
    });
    surfaceRoughness.fill(instance.sceneRoughness, firstVertex, firstVertex + vertexCount);
    opaque.set(vertices, opaqueOffset);
    opaqueOffset+= vertices.length;
  }

  return {
    opaque,
    shadowCasters,
    opaqueRanges,
    surfaceRoughness,
    transparent: sortCameraTransparentTriangles(transparent, backward),
    visible: queuePlan.visibleCount,
    culled: queuePlan.culledCount
  };
}

/** Determine whether an instance belongs in the transparent queue. */
function isTranslucent(instance: MeshInstance): boolean {
  const usesTransparentPass = instance.pass === 'transparent';
  const usesAutomaticTransparency = instance.pass === 'auto'
    && ((instance.opacity < 1) || instance.asset.hasTransparency);

  return usesTransparentPass || usesAutomaticTransparency;
}

/** Validate an opaque instance's normalized surface roughness. */
function validateSurfaceRoughness(surfaceRoughness: number): void {
  const outsideRange = (surfaceRoughness < MINIMUM_SURFACE_ROUGHNESS)
    || (surfaceRoughness > MAXIMUM_SURFACE_ROUGHNESS);

  if(!Number.isFinite(surfaceRoughness) || outsideRange) {
    throw new RangeError('Scene roughness must be between zero and one.');
  }
}
