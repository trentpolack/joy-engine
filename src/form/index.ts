// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/** Renderer-independent procedural content authoring and runtime evaluation. */
export { compile, compileFormScript } from './compiler.ts';
export { parse, ScriptError } from './parser.ts';
export { createParameter, scalar, readParameterValue } from './parameters.ts';
export { defaultMaterial, readMaterials } from './materials.ts';
export { DOCUMENT_LIMITS } from './document-limits.ts';
export { meshBox, meshSphere, materialPbr, readMesh, readMaterialValue, transformMesh } from './resources.ts';
export type { Parameter } from './parameters.ts';
export type { ParameterValue } from './parameters.ts';
export type { GeometryData } from './geometry.ts';
export type { Material } from './materials.ts';
export type { MaterialTextures } from './materials.ts';
export type { ProjectMetadata } from './parser.ts';
export type { Token } from './parser.ts';
export type { MeshValue } from './resources.ts';
export type { MaterialValue } from './resources.ts';
export type { NumericParameter } from './parameters.ts';
export type { ResourceParameter } from './parameters.ts';

export type { GeometryValue } from './procedural-geometry.ts';
export type { InstanceValue } from './procedural-geometry.ts';
export type { Domain as AttributeDomain } from './procedural-geometry.ts';
export type { NamedOutput } from './procedural-output.ts';

export type { DataValue } from './user-data.ts';
export type { UserData } from './user-data.ts';

export { createFormMeshAsset } from './mesh-asset.ts';
