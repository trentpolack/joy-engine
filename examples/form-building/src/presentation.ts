// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import {MeshAsset, RenderScene} from 'joy-engine';
import {readMaterials} from 'joy-engine/form';

/**
 * Adapt FORM's material groups to SceneRenderer's supported tint/roughness channels.
 * This is a presentation adapter, not geometry generation. The canonical asset stays unchanged.
 * Metallic, emissive, textures, and material alpha policy are not interpreted by SceneRenderer.
 */
export function populateBuildingScene(scene:RenderScene, asset:MeshAsset) {
  const geometry = asset.geometry;
  const materials = readMaterials(geometry.materials);
  const groups = new Map<number,number[]>();
  for(let triangle = 0; triangle < geometry.indices.length/3; triangle++) {
    const materialIndex = geometry.triangleMaterials[triangle] ?? 0;
    let indices = groups.get(materialIndex);
    if(!indices) {
      indices = [];
      groups.set(materialIndex,indices);
    }
    indices.push(...geometry.indices.slice(triangle*3,triangle*3 + 3));
  }
  // Prepare every group before touching live scene state. Failed preparation keeps the old scene.
  const prepared = [...groups].map(([materialIndex,indices]) => ({
    asset:new MeshAsset({...geometry,indices,triangleMaterials:Array(indices.length/3).fill(materialIndex)}),
    material:materials[materialIndex]
  }));
  scene.clear();
  for(const group of prepared) {
    scene.createMesh(group.asset,{tint:group.material.baseColor.slice(0,3),sceneRoughness:group.material.roughness});
  }
  return prepared.length;
}
