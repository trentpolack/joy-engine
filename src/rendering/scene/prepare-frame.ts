// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { RenderScene } from './render-scene.ts';
import type { MeshInstance } from './mesh-instance.ts';
import type { PreparedMesh } from './mesh-frame.ts';

import { sortCameraTransparentTriangles } from '../camera/orbit-camera.ts';
import { outsideFrustum } from './mesh-transform.ts';
import { prepareMesh } from './mesh-frame.ts';

// Interleaved XYZ/RGBA floats per vertex.
const FLOATS_PER_VERTEX = 7;

/** One visible instance's destination, decided before any copying. */
interface RoutedMesh {
  instance: MeshInstance;
  prepared: PreparedMesh;
  translucent: boolean;
  culled: boolean;
}

/**
 * Prepare owned vertex-color queues without mutating scene, assets or borrowed triangles.
 * Authored normals/UVs/materials stay on MeshAsset for other adapters; this adapter matches
 * the existing flat-lit colored-triangle games. Transparency uses global centroid sorting.
 *
 * Queues are assembled in two passes: route and size every instance, then copy each
 * cached Float32Array once with `TypedArray.set`. Order matches submission order:
 * borrowed triangles first, then instances in scene order. Opaque queues and shadow
 * casters are caller-owned Float32Arrays; roughness and sorted transparency are numbers.
 * @param scene
 * @param options
 */
export function prepareSceneFrame(scene: RenderScene, {backward, viewProjection, webgpu = false, opaqueTriangles = [], transparentTriangles = []}: {
    backward: readonly number[];
    viewProjection?: ArrayLike<number>;
    webgpu?: boolean;
    opaqueTriangles?: readonly number[];
    transparentTriangles?: readonly number[];
}) {
  scene.assertAlive();

  // Pass 1: prepare (or reuse) instance geometry, route it, and size every queue.
  const routed: RoutedMesh[] = [];
  let opaqueLength = opaqueTriangles.length;
  let casterLength = opaqueTriangles.length;
  let transparentLength = transparentTriangles.length;
  let visible = 0;
  let culled = 0;
  for(const instance of scene.instances) {
    if(!instance.visible || instance.opacity === 0) {
      continue;
    }
    const prepared = prepareMesh(instance, scene.frameCache);
    const translucent = instance.pass === 'transparent' || (instance.pass === 'auto' && (instance.opacity < 1 || instance.asset.hasTransparency));
    if(!translucent) {
      casterLength+= prepared.vertices.length;
    }
    const outside = Boolean(viewProjection && outsideFrustum(instance.asset, prepared.transform, viewProjection, webgpu));
    if(outside) {
      culled++;
    } else {
      visible++;
      if(translucent) {
        transparentLength+= prepared.vertices.length;
      } else {
        if(!Number.isFinite(instance.sceneRoughness) || instance.sceneRoughness < 0 || instance.sceneRoughness > 1) {
          throw new RangeError('Scene roughness must be between zero and one.');
        }
        opaqueLength+= prepared.vertices.length;
      }
    }
    routed.push({instance, prepared, translucent, culled: outside});
  }

  // Pass 2: copy into exactly sized owned arrays. Borrowed transient triangles lead each queue.
  const opaque = new Float32Array(opaqueLength);
  const shadowCasters = new Float32Array(casterLength);
  const transparent = new Float32Array(transparentLength);
  // Roughness keeps exact authored values; one entry per vertex is cheap to fill.
  const surfaceRoughness: number[] = new Array(opaqueLength/FLOATS_PER_VERTEX).fill(1);
  const opaqueRanges = [];
  opaque.set(opaqueTriangles);
  shadowCasters.set(opaqueTriangles);
  transparent.set(transparentTriangles);
  let opaqueOffset = opaqueTriangles.length;
  let casterOffset = opaqueTriangles.length;
  let transparentOffset = transparentTriangles.length;

  for(const {instance, prepared, translucent, culled: outside} of routed) {
    const vertices = prepared.vertices;
    // Camera-culled opaque geometry still casts shadows into the view.
    if(!translucent) {
      shadowCasters.set(vertices, casterOffset);
      casterOffset+= vertices.length;
    }
    if(outside) {
      continue;
    }
    if(translucent) {
      transparent.set(vertices, transparentOffset);
      transparentOffset+= vertices.length;
      continue;
    }
    const firstVertex = opaqueOffset/FLOATS_PER_VERTEX;
    const vertexCount = vertices.length/FLOATS_PER_VERTEX;
    opaqueRanges.push({identity: instance, topology: instance.asset, firstVertex, vertexCount});
    surfaceRoughness.fill(instance.sceneRoughness, firstVertex, firstVertex + vertexCount);
    opaque.set(vertices, opaqueOffset);
    opaqueOffset+= vertices.length;
  }

  return {opaque, shadowCasters, opaqueRanges, surfaceRoughness, transparent: sortCameraTransparentTriangles(transparent, backward), visible, culled};
}
