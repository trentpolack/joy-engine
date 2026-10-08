// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { RenderScene } from './render-scene.ts';

import { sortCameraTransparentTriangles } from '../camera/orbit-camera.ts';
import { outsideFrustum } from './mesh-transform.ts';
import { prepareMesh } from './mesh-frame.ts';

/** Prepare owned vertex-color queues without mutating scene, assets or borrowed triangles.
 * Authored normals/UVs/materials stay on MeshAsset for other adapters; this adapter matches
 * the existing flat-lit colored-triangle games. Transparency uses global centroid sorting.
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
  const opaque = Array.from(opaqueTriangles);
  const transparent = Array.from(transparentTriangles);
  const opaqueRanges = [];
  const surfaceRoughness = Array(opaque.length/7).fill(1) as number[];
  let visible = 0;
  let culled = 0;
  for(const instance of scene.instances) {
    if(!instance.visible || instance.opacity === 0) {
      continue;
    }
    const prepared = prepareMesh(instance, scene.frameCache);
    const transform = prepared.transform;
    if(viewProjection && outsideFrustum(instance.asset, transform, viewProjection, webgpu)) {
      culled++;
      continue;
    }
    visible++;
    const translucent = instance.pass === 'transparent' || (instance.pass === 'auto' && (instance.opacity < 1 || instance.asset.hasTransparency));
    const target = translucent ? transparent : opaque;
    if(!translucent) {
      if(!Number.isFinite(instance.sceneRoughness) || instance.sceneRoughness < 0 || instance.sceneRoughness > 1) {
        throw new RangeError('Scene roughness must be between zero and one.');
      }
      opaqueRanges.push({identity: instance, topology: instance.asset, firstVertex: opaque.length/7, vertexCount: prepared.vertices.length/7});
      for(let vertex = 0; vertex < prepared.vertices.length/7; vertex++) {
        surfaceRoughness.push(instance.sceneRoughness);
      }
    }
    for(const value of prepared.vertices) {
      target.push(value);
    }
  }
  return {opaque, opaqueRanges, surfaceRoughness, transparent: sortCameraTransparentTriangles(transparent, backward), visible, culled};
}
