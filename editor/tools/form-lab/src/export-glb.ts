// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { FORM_MATERIAL, MATERIAL_ALPHA_MODE } from 'joy-engine/constants';
import type { Material } from 'joy-engine/form';
import type { GLTFMaterial } from '@loaders.gl/gltf';
import { readMaterials } from 'joy-engine/form';

export interface GlbGeometry {
  positions: number[];
  normals?: number[];
  uvs?: number[];
  colors: number[];
  alphas: number[];
  triangles: number[];
  points: number[];
  materials: Material[];
  triangleMaterials: number[];
  pointMaterials: number[];
}

const GLB_MAGIC = 0x46546c67;
const JSON_CHUNK = 0x4e4f534a;
const BINARY_CHUNK = 0x004e4942;

/** Export authored triangle and point geometry as a self-contained glTF 2.0 binary. @param data */
export function exportGlb(data: GlbGeometry) {
  validateGeometry(data);
  if(!Array.isArray(data.materials) || data.materials[0]?.name !== FORM_MATERIAL.DEFAULT) {
    throw new Error("GLB materials must contain 'Default' at index 0.");
  }
  const materials = readMaterials(data.materials);
  validateAssignments(data, materials.length);

  const bufferViews: object[] = [];

  const accessors: object[] = [];

  const binaryParts: Uint8Array[] = [];
  let binaryLength = 0;

  /** @param values @param componentCount @param includeBounds */
  function addAccessor(values: number[], componentCount: 2 | 3 | 4, includeBounds: boolean) {
    const floatValues = new Float32Array(values);
    const bytes = new Uint8Array(floatValues.buffer);
    const bufferView = bufferViews.length;
    bufferViews.push({ buffer: 0, byteOffset: binaryLength, byteLength: bytes.byteLength, target: 34962 });
    binaryParts.push(bytes);
    binaryLength += bytes.byteLength;
    const accessor = {
      bufferView,
      componentType: 5126,
      count: values.length / componentCount,
      type: `VEC${componentCount}`,
    };
    if(includeBounds) {
      Object.assign(accessor, bounds(floatValues, componentCount));
    }
    accessors.push(accessor);
    return accessors.length - 1;
  }

  const primitives = [];
  for(const [material, triangleIndexes] of groupAssignments(data.triangleMaterials)) {
    const positions = [];
    const normals = [];
    const colors = [];
    for(const triangleIndex of triangleIndexes) {
      const indexes = data.triangles.slice(triangleIndex * 3, triangleIndex * 3 + 3);
      const faceNormal = flatNormal(data.positions, indexes);
      for(const index of indexes) {
        positions.push(...data.positions.slice(index * 3, index * 3 + 3));
        const authoredNormal = data.normals?.slice(index * 3, index * 3 + 3);
        const normal = authoredNormal?.some(value => value !== 0) ? authoredNormal : faceNormal;
        const length = Math.hypot(...normal);
        normals.push(...normal.map(value => value / length));
        colors.push(...data.colors.slice(index * 3, index * 3 + 3), data.alphas[index]);
      }
    }
    const textureCoordinates = data.uvs?.length === data.positions.length / 3 * 2
      ? triangleIndexes.flatMap((triangleIndex) =>
          data.triangles
            .slice(triangleIndex * 3, triangleIndex * 3 + 3)
            .flatMap((index) =>
               ((data.uvs) as number[]).slice(index * 2, index * 2 + 2),
            ),
        )
      : null;
    primitives.push({
      attributes: {
        POSITION: addAccessor(positions, 3, true),
        NORMAL: addAccessor(normals, 3, false),
        COLOR_0: addAccessor(colors, 4, false),
        ...(textureCoordinates
          ? { TEXCOORD_0: addAccessor(textureCoordinates, 2, false) }
          : {}),
      },
      mode: 4,
      material,
    });
  }
  for(const [material, pointIndexes] of groupAssignments(data.pointMaterials)) {
    const positions = [];
    const colors = [];
    for(const assignmentIndex of pointIndexes) {
      const index = data.points[assignmentIndex];
      positions.push(...data.positions.slice(index * 3, index * 3 + 3));
      colors.push(...data.colors.slice(index * 3, index * 3 + 3), data.alphas[index]);
    }
    primitives.push({
      attributes: {
        POSITION: addAccessor(positions, 3, true),
        COLOR_0: addAccessor(colors, 4, false),
      },
      mode: 0,
      material,
    });
  }

  const images: object[] = [];

  const textures: object[] = [];
  const textureIndexes = new Map();
  let usesWebp = false;
  /** @param source */
  const textureIndex = (source: string | null) => {
    if(!source) {
      return undefined;
    }
    const existing = textureIndexes.get(source);
    if(existing !== undefined) {
      return existing;
    }
    const match = /^data:(image\/(?:png|jpeg|webp));base64,(.+)$/.exec(source);
    if(!match) {
      throw new Error('GLB textures must be PNG, JPEG, or WebP data URLs.');
    }
    const bytes = Uint8Array.from(atob(match[2]), character => character.charCodeAt(0));
    const bufferView = bufferViews.length;
    bufferViews.push({ buffer: 0, byteOffset: binaryLength, byteLength: bytes.byteLength });
    binaryParts.push(bytes);
    binaryLength += bytes.byteLength;
    const index = textures.length;
    images.push({ bufferView, mimeType: match[1] });
    if(match[1] === 'image/webp') {
      usesWebp = true;
      textures.push({ extensions: { EXT_texture_webp: { source: images.length - 1 } } });
    } else {
      textures.push({ source: images.length - 1 });
    }
    textureIndexes.set(source, index);
    return index;
  };
  const gltf = {
    asset: { version: '2.0', generator: 'FORM LAB' },
    scene: 0,
    scenes: [{ nodes: [0] }],
    nodes: [{ mesh: 0, name: 'FormLab' }],
    meshes: [{ name: 'FormLab', primitives }],
    materials: materials.map(material => exportMaterial(material, textureIndex)),
    buffers: [{ byteLength: binaryLength }],
    bufferViews,
    accessors,
    ...(images.length ? { images, textures } : {}),
    ...(usesWebp ? { extensionsUsed: ['EXT_texture_webp'], extensionsRequired: ['EXT_texture_webp'] } : {}),
  };
  return writeGlb(gltf, binaryParts, binaryLength);
}

/** @param data */
function validateGeometry(data: GlbGeometry) {
  if(data.triangles.length === 0 && data.points.length === 0) {
    throw new Error('Cannot export empty geometry as GLB.');
  }
  const vertexCount = data.positions.length / 3;
  if(!Number.isInteger(vertexCount) || data.colors.length !== vertexCount * 3 || data.alphas.length !== vertexCount) {
    throw new Error('GLB vertex position, color, and alpha arrays must describe the same vertices.');
  }
  if(data.triangles.length % 3 !== 0) {
    throw new Error('GLB triangle topology must contain complete triangles.');
  }
  if(data.normals && data.normals.length !== data.positions.length) {
    throw new Error('GLB normals must describe the same vertices as positions.');
  }
  if(data.uvs && data.uvs.length !== vertexCount * 2) {
    throw new Error('GLB texture coordinates must describe the same vertices as positions.');
  }
  for(const value of [...data.positions, ...data.colors, ...data.alphas, ...(data.normals ?? []), ...(data.uvs ?? [])]) {
    if(!Number.isFinite(value)) {
      throw new Error('GLB vertex data must contain only finite numbers.');
    }
  }
  for(const position of data.positions) {
    if(!Number.isFinite(Math.fround(position))) {
      throw new Error('GLB positions must be representable as float32 values.');
    }
  }
  for(const color of [...data.colors, ...data.alphas]) {
    if(color < 0 || color > 1) {
      throw new Error('GLB vertex colors and alpha must be from 0 to 1.');
    }
  }
  for(const index of [...data.triangles, ...data.points]) {
    if(!Number.isInteger(index) || index < 0 || index >= vertexCount) {
      throw new Error(`GLB topology contains invalid vertex index '${index}'.`);
    }
  }
}

/** @param data @param materialCount */
function validateAssignments(data: GlbGeometry, materialCount: number) {
  if(data.triangleMaterials.length !== data.triangles.length / 3 || data.pointMaterials.length !== data.points.length) {
    throw new Error('GLB material assignment counts must match triangle and point topology.');
  }
  for(const index of [...data.triangleMaterials, ...data.pointMaterials]) {
    if(!Number.isInteger(index) || index < 0 || index >= materialCount) {
      throw new Error(`GLB topology contains invalid material index '${index}'.`);
    }
  }
}

/** @param assignments */
function groupAssignments(assignments: number[]): Map<number, number[]> {

  const groups: Map<number, number[]> = new Map();
  for(const [index, material] of assignments.entries()) {
    const indexes = groups.get(material) ?? [];
    indexes.push(index);
    groups.set(material, indexes);
  }
  return groups;
}

/** @param positions @param indexes */
function flatNormal(positions: number[], indexes: number[]) {
  const [a, b, c] = indexes.map((index) =>
    positions.slice(index * 3, index * 3 + 3).map(Math.fround),
  );
  const ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
  const ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
  const normal = [
    ab[1] * ac[2] - ab[2] * ac[1],
    ab[2] * ac[0] - ab[0] * ac[2],
    ab[0] * ac[1] - ab[1] * ac[0],
  ];
  const length = Math.hypot(...normal);
  if(length === 0) {
    throw new Error('GLB export does not support degenerate triangles.');
  }
  return normal.map((value) => value / length);
}

/** @param values @param componentCount */
function bounds(values: Float32Array, componentCount: number) {
  const min = Array(componentCount).fill(Infinity);
  const max = Array(componentCount).fill(-Infinity);
  for(let index = 0; index < values.length; index++) {
    const component = index % componentCount;
    min[component] = Math.min(min[component], values[index]);
    max[component] = Math.max(max[component], values[index]);
  }
  return { min, max };
}

/** @param material @param textureIndex */
function exportMaterial(material: Material, textureIndex: (source: string | null) => number | undefined) {

  const result: GLTFMaterial & {pbrMetallicRoughness: NonNullable<GLTFMaterial['pbrMetallicRoughness']>} = {
    name: material.name,
    pbrMetallicRoughness: {
      baseColorFactor: material.baseColor,
      metallicFactor: material.metallic,
      roughnessFactor: material.roughness,
    },
    emissiveFactor: material.emissive,
    alphaMode: material.alphaMode,
    doubleSided: true,
  };
  const baseColor = textureIndex(material.textures.baseColor);
  const normal = textureIndex(material.textures.normal);
  const emissive = textureIndex(material.textures.emissive);
  const metallic = textureIndex(material.textures.metallic);
  const roughness = textureIndex(material.textures.roughness);
  const occlusion = textureIndex(material.textures.ao);
  if(baseColor !== undefined) {
    result.pbrMetallicRoughness.baseColorTexture = { index: baseColor };
  }
  if(metallic !== undefined || roughness !== undefined) {
    result.pbrMetallicRoughness.metallicRoughnessTexture = { index: metallic ?? roughness! };
  }
  if(normal !== undefined) {
    result.normalTexture = { index: normal };
  }
  if(emissive !== undefined) {
    result.emissiveTexture = { index: emissive };
  }
  if(occlusion !== undefined) {
    result.occlusionTexture = { index: occlusion };
  }
  if(material.alphaMode === MATERIAL_ALPHA_MODE.MASK) {
    Object.assign(result, { alphaCutoff: material.alphaCutoff });
  }
  return result;
}

/** @param gltf @param binaryParts @param binaryLength */
function writeGlb(gltf: object, binaryParts: Uint8Array[], binaryLength: number) {
  const jsonSource = JSON.stringify(gltf);
  const unpaddedJson = new TextEncoder().encode(jsonSource);
  const jsonLength = align4(unpaddedJson.byteLength);
  const paddedBinaryLength = align4(binaryLength);
  const totalLength = 12 + 8 + jsonLength + 8 + paddedBinaryLength;
  const output = new ArrayBuffer(totalLength);
  const view = new DataView(output);
  view.setUint32(0, GLB_MAGIC, true);
  view.setUint32(4, 2, true);
  view.setUint32(8, totalLength, true);
  view.setUint32(12, jsonLength, true);
  view.setUint32(16, JSON_CHUNK, true);
  const jsonBytes = new Uint8Array(output, 20, jsonLength);
  jsonBytes.fill(0x20);
  jsonBytes.set(unpaddedJson);
  const binaryHeader = 20 + jsonLength;
  view.setUint32(binaryHeader, paddedBinaryLength, true);
  view.setUint32(binaryHeader + 4, BINARY_CHUNK, true);
  const binaryBytes = new Uint8Array(output, binaryHeader + 8, paddedBinaryLength);
  let offset = 0;
  for(const part of binaryParts) {
    binaryBytes.set(part, offset);
    offset += part.byteLength;
  }
  return output;
}

/** @param value */
function align4(value: number) {
  return Math.ceil(value / 4) * 4;
}

