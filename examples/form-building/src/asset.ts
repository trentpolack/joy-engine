// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import { MeshAsset } from 'joy-engine';
import type { GeometryData } from 'joy-engine/form';

/** Copy a completed FORM evaluation into an immutable mesh; scene instances borrow it. */
export function assetFromEvaluation(result:GeometryData) {
  return new MeshAsset({positions:result.positions, indices:result.triangles, normals:result.normals,
    uvs:result.uvs, colors:result.colors, alphas:result.alphas, materials:result.materials,
    triangleMaterials:result.triangleMaterials, metadata:{project:result.project, userData:result.userData}});
}
