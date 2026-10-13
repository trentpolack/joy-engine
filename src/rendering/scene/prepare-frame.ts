// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { RenderScene } from './render-scene.ts';

import { sortCameraTransparentTriangles } from '../camera/orbit-camera.ts';
import { outsideFrustum } from './mesh-transform.ts';
import type { PreparedSceneGeometry, PreparedTerrainPacket } from './scene-geometry.ts';
import { SCENE_GEOMETRY_KIND, prepareSceneGeometry } from './scene-geometry.ts';
import { MESH_PASS } from './mesh-instance.ts';
import { prepareMesh } from './mesh-frame.ts';

/** Prepare owned vertex-color queues without mutating scene, assets or borrowed triangles.
 * Authored normals/UVs/materials stay on MeshAsset for other adapters; this adapter matches
 * the existing flat-lit colored-triangle games. Transparency uses global centroid sorting.
 * @param scene
 * @param options
 * @param preparedGeometry Optional same-scene snapshots from this frame, reused after capability validation.
 */
export function prepareSceneFrame(scene: RenderScene, {backward, viewProjection, webgpu = false, opaqueTriangles = [], transparentTriangles = []}: {
    backward: readonly number[];
    viewProjection?: ArrayLike<number>;
    webgpu?: boolean;
    opaqueTriangles?: readonly number[];
    transparentTriangles?: readonly number[];
}, preparedGeometry?: readonly PreparedSceneGeometry[]) {
  scene.assertAlive();
  const opaque = Array.from(opaqueTriangles);
  const shadowCasters = Array.from(opaqueTriangles);
  const transparent = Array.from(transparentTriangles);
  const opaqueRanges = [];
  const receivesShadowRanges: {firstVertex: number; vertexCount: number; receivesShadow: boolean}[] = [];
  if(opaque.length) {
    receivesShadowRanges.push({firstVertex: 0, vertexCount: opaque.length/7, receivesShadow: true});
  }
  const geometrySnapshots = preparedGeometry ?? prepareSceneGeometry(scene);
  const surfaceRoughness = Array(opaque.length/7).fill(1) as number[];
  const terrainPackets: PreparedTerrainPacket[] = [];
  let visible = 0;
  let culled = 0;
  for(const geometry of geometrySnapshots) {
    if(geometry.kind === SCENE_GEOMETRY_KIND.TERRAIN) {
      const cameraVisible = !viewProjection || !outsideFrustum({bounds: geometry.localBounds}, geometry.worldTransform, viewProjection, webgpu);
      terrainPackets.push({...geometry, visible: cameraVisible});
      if(cameraVisible) {
        visible++;
      } else {
        culled++;
      }
      continue;
    }
    const instance = geometry.instance;
    const prepared = prepareMesh(instance, scene.frameCache, geometry.worldTransform);
    const translucent = geometry.coverage === MESH_PASS.TRANSPARENT;
    if(geometry.castsShadow) {
      for(const value of prepared.vertices) {
        shadowCasters.push(value);
      }
    }
    const transform = prepared.transform;
    if(viewProjection && outsideFrustum(instance.asset, transform, viewProjection, webgpu)) {
      culled++;
      continue;
    }
    visible++;
    const target = translucent ? transparent : opaque;
    if(!translucent) {
      if(!Number.isFinite(instance.sceneRoughness) || instance.sceneRoughness < 0 || instance.sceneRoughness > 1) {
        throw new RangeError('Scene roughness must be between zero and one.');
      }
      opaqueRanges.push({identity: instance, topology: instance.asset, firstVertex: opaque.length/7, vertexCount: prepared.vertices.length/7});
      receivesShadowRanges.push({firstVertex: opaque.length/7, vertexCount: prepared.vertices.length/7, receivesShadow: geometry.receivesShadow});
      for(let vertex = 0; vertex < prepared.vertices.length/7; vertex++) {
        surfaceRoughness.push(instance.sceneRoughness);
      }
    }
    for(const value of prepared.vertices) {
      target.push(value);
    }
  }
  return {opaque, shadowCasters, opaqueRanges, receivesShadowRanges, geometrySnapshots, terrainPackets, surfaceRoughness, transparent: sortCameraTransparentTriangles(transparent, backward), visible, culled};
}
