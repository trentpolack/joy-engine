// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { ParameterValue } from './parameters.ts';

import { compile } from './compiler.ts';
import { MeshAsset } from '../rendering/scene/mesh-asset.ts';

/** Compile FORM into an immutable local-space mesh asset, preserving authoring data.
 * The caller owns the asset reference; scenes borrow it. Evaluation does not alter live instances.
 * @param source @param [overrides]
 */
export function createFormMeshAsset(source: string, overrides?: Record<string, ParameterValue>) {
  const result = compile(source, overrides);
  return new MeshAsset({
    positions: result.positions, indices: result.triangles, normals: result.normals,
    uvs: result.uvs, colors: result.colors, alphas: result.alphas,
    materials: result.materials, triangleMaterials: result.triangleMaterials,
    metadata: {outputs: result.outputs ?? [], parameters: result.parameters, userData: result.userData ?? [],
      points: result.points, pointMaterials: result.pointMaterials, project: result.project}
  });
}
