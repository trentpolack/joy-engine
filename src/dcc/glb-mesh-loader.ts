// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { IndexedMesh3D } from '../core/types.ts';
import type { GLTFPostprocessed } from '@loaders.gl/gltf';

import { load, parse } from '@loaders.gl/core';
import { GLTFLoader, postProcessGLTF } from '@loaders.gl/gltf';

export type { IndexedMesh3D };
export type { GLTFPostprocessed };

const TRIANGLES_MODE = 4;
const FLOAT_COMPONENT = 5126;
const UNSIGNED_BYTE_COMPONENT = 5121;
const UNSIGNED_SHORT_COMPONENT = 5123;
const UNSIGNED_INT_COMPONENT = 5125;
const VEC3_COMPONENTS = 3;

const GEOMETRY_OPTIONS = {
  gltf: {
    loadImages: false
  }
};

/**
 * Load one indexed triangle mesh from a binary or JSON glTF 2.0 asset.
 * Linked buffers in JSON glTF assets are resolved relative to the asset URL.
 * Returns copied model-space data; callers own projection, materials, and lighting.
 * @param url
 * @param [meshName] Exact glTF mesh name to decode.
 * @returns Model-space data owned by the caller.
 */
export async function loadGlbMesh(url: string, meshName?: string): Promise<IndexedMesh3D> {
  const gltf = await load(url, GLTFLoader, GEOMETRY_OPTIONS);
  return createIndexedMesh(postProcessGLTF(gltf), meshName);
}

/**
 * Decode one indexed triangle mesh from in-memory binary glTF 2.0 data.
 * A mesh name can select one object from exporters that retain other scene objects.
 * loaders.gl owns container, buffer, accessor, sparse-data, and stride decoding.
 * Unindexed triangles receive sequential indices. Scene/node transforms, materials, animation, and additional primitives are intentionally not interpreted.
 * @param buffer
 * @param [meshName]
 * @returns Copied model-space data owned by the caller.
 */
export async function parseGlbMesh(buffer: ArrayBuffer, meshName?: string): Promise<IndexedMesh3D> {
  const gltf = await parse(buffer, GLTFLoader, GEOMETRY_OPTIONS);
  return createIndexedMesh(postProcessGLTF(gltf), meshName);
}

/**
 * Copy one decoded glTF primitive into Joy Engine's renderer-independent mesh record.
 * @param gltf
 * @param [meshName]
 */
function createIndexedMesh(gltf: GLTFPostprocessed, meshName?: string): IndexedMesh3D {
  const mesh = meshName
    ? gltf.meshes?.find((candidate) => candidate.name === meshName)
    : gltf.meshes?.[0];
  if(!mesh) {
    throw new Error(meshName
      ? `The glTF asset does not contain the requested mesh "${meshName}".`
      : 'The glTF asset does not contain a mesh.');
  }
  if(mesh.primitives.length !== 1) {
    throw new Error(`The glTF mesh "${mesh.name ?? 'mesh'}" must contain exactly one primitive.`);
  }

  const primitive = mesh.primitives[0];
  const positionAccessor = primitive?.attributes.POSITION;
  const normalAccessor = primitive?.attributes.NORMAL;
  const indexAccessor = primitive?.indices;
  const hasSupportedIndices = indexAccessor && [
    UNSIGNED_BYTE_COMPONENT,
    UNSIGNED_SHORT_COMPONENT,
    UNSIGNED_INT_COMPONENT
  ].includes(indexAccessor.componentType);
  if(!primitive || (primitive.mode !== undefined && primitive.mode !== TRIANGLES_MODE) ||
      positionAccessor?.componentType !== FLOAT_COMPONENT ||
      positionAccessor?.components !== VEC3_COMPONENTS ||
      normalAccessor?.componentType !== FLOAT_COMPONENT ||
      normalAccessor?.components !== VEC3_COMPONENTS ||
      (indexAccessor && !hasSupportedIndices)) {
    throw new Error('The glTF asset must contain a triangle mesh with float positions/normals and optional unsigned indices.');
  }

  const positions = Array.from(positionAccessor.value);
  const normals = Array.from(normalAccessor.value);
  // FORM and other exporters may emit unindexed triangles; normalize them to
  // the same owned indexed contract used by runtime mesh consumers.
  const indices = indexAccessor
    ? Array.from(indexAccessor.value)
    : Array.from({length: positions.length/VEC3_COMPONENTS}, (_, index) => index);
  if(positions.length === 0 || indices.length === 0) {
    throw new Error('The glTF mesh must contain at least one indexed triangle.');
  }
  if(positions.length !== normals.length) {
    throw new Error('The glTF mesh must contain matching position and normal counts.');
  }
  if(indices.length % 3 !== 0) {
    throw new Error('The glTF mesh indices must describe complete triangles.');
  }
  if(!positions.every(Number.isFinite) || !normals.every(Number.isFinite)) {
    throw new Error('The glTF mesh must contain finite position and normal values.');
  }
  const vertexCount = positions.length/VEC3_COMPONENTS;
  if(indices.some((index) => index >= vertexCount)) {
    throw new Error('The glTF mesh contains an index outside the vertex range.');
  }

  return {
    name: mesh.name ?? 'mesh',
    positions,
    normals,
    indices
  };
}

