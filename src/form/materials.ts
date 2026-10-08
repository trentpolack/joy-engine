// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { MATERIAL_ALPHA_MODE } from '../rendering/pbr/constants.ts';
import { FORM_MATERIAL } from './constants.ts';
import { DOCUMENT_LIMITS } from './document-limits.ts';

export type AlphaMode = typeof MATERIAL_ALPHA_MODE.OPAQUE | typeof MATERIAL_ALPHA_MODE.MASK | typeof MATERIAL_ALPHA_MODE.BLEND;
export interface Material {
  name: string;
  baseColor: number[];
  metallic: number;
  roughness: number;
  emissive: number[];
  alphaMode: AlphaMode;
  alphaCutoff: number;
  textures: MaterialTextures;
}
export interface MaterialTextures {
  baseColor: string | null;
  normal: string | null;
  emissive: string | null;
  metallic: string | null;
  roughness: string | null;
  ao: string | null;
}

const MAX_MATERIALS = 128;
const MATERIAL_FIELDS = new Set([
  'name',
  'baseColor',
  'metallic',
  'roughness',
  'emissive',
  'alphaMode',
  'alphaCutoff',
  'textures',
]);

/** Return a new copy of FORM LAB's editable default material. */
export function defaultMaterial(): Material {
  return {
    name: FORM_MATERIAL.DEFAULT,
    baseColor: [1, 1, 1, 1],
    metallic: 0,
    roughness: 1,
    emissive: [0, 0, 0],
    alphaMode: MATERIAL_ALPHA_MODE.OPAQUE,
    alphaCutoff: 0.5,
    textures: emptyTextures(),
  };
}

/**
 * Validate serialized material data and return owned records with Default first.
 * @param value
 */
export function readMaterials(value: unknown): Material[] {
  if(value === undefined) {
    return [defaultMaterial()];
  }
  if(!Array.isArray(value)) {
    throw new Error('Materials must be an array.');
  }

  const materials: Material[] = [];
  const names = new Set();
  let totalTextureBytes = 0;
  for(const [index, candidate] of value.entries()) {
    const material = readMaterial(candidate, index);
    for(const source of Object.values(material.textures)) {
      if(source !== null) {
        totalTextureBytes+= textureByteLength(source);
      }
    }
    if(totalTextureBytes > DOCUMENT_LIMITS.totalTextureBytes) {
      throw new Error('Combined material textures must be 32 MB or smaller.');
    }
    if(names.has(material.name)) {
      throw new Error(`Material names must be unique; '${material.name}' is duplicated.`);
    }
    names.add(material.name);
    materials.push(material);
  }

  const defaultIndex = materials.findIndex((material) => material.name === FORM_MATERIAL.DEFAULT);
  if(defaultIndex < 0) {
    materials.unshift(defaultMaterial());
  } else if(defaultIndex > 0) {
    const [existingDefault] = materials.splice(defaultIndex, 1);
    materials.unshift(existingDefault);
  }
  if(materials.length > MAX_MATERIALS) {
    throw new Error(`At most ${MAX_MATERIALS} materials are supported.`);
  }
  return materials;
}

/** @param value @param index */
function readMaterial(value: unknown, index: number): Material {
  if(value === null || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error(`Material ${index} must be an object.`);
  }
  const record = ((value) as Record<string, unknown>);
  for(const field of Object.keys(record)) {
    if(!MATERIAL_FIELDS.has(field)) {
      throw new Error(`Material '${String(record.name ?? index)}' has unsupported field '${field}'.`);
    }
  }
  if(typeof record.name !== 'string' || !/^[A-Za-z_][A-Za-z0-9_]{0,63}$/.test(record.name)) {
    throw new Error(`Material ${index} name must be an ASCII identifier of at most 64 characters.`);
  }
  const baseColor = readTuple(record.baseColor, 4, `${record.name}.baseColor`);
  const emissive = readTuple(record.emissive, 3, `${record.name}.emissive`);
  const metallic = readUnitNumber(record.metallic, `${record.name}.metallic`);
  const roughness = readUnitNumber(record.roughness, `${record.name}.roughness`);
  const alphaCutoff = readUnitNumber(record.alphaCutoff, `${record.name}.alphaCutoff`);
  const textures = readTextures(record.textures, String(record.name));
  if(!Object.values(MATERIAL_ALPHA_MODE).some(mode => mode === String(record.alphaMode))) {
    throw new Error(`${record.name}.alphaMode must be OPAQUE, MASK, or BLEND.`);
  }
  return {
    name: record.name,
    baseColor,
    metallic,
    roughness,
    emissive,
    alphaMode:  ((record.alphaMode) as AlphaMode),
    alphaCutoff,
    textures,
  };
}

function emptyTextures(): MaterialTextures {
  return { baseColor: null, normal: null, emissive: null, metallic: null, roughness: null, ao: null };
}

/** @param value @param name */
function readTextures(value: unknown, name: string): MaterialTextures {
  if(value === undefined) {
    return emptyTextures();
  }
  if(value === null || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error(`${name}.textures must be an object.`);
  }
  const result = emptyTextures();
  for(const [key, source] of Object.entries(value)) {
    if(!Object.hasOwn(result, key) || (source !== null && typeof source !== 'string')) {
      throw new Error(`${name}.textures contains an invalid '${key}' texture.`);
    }
    if(typeof source === 'string') {
      if(!/^data:image\/(png|jpeg|webp);base64,/.test(source)) {
        throw new Error(`${name}.${key} must be an embedded PNG, JPEG, or WebP texture.`);
      }
      if(textureByteLength(source) > DOCUMENT_LIMITS.textureBytes) {
        throw new Error('Textures must be 4 MB or smaller.');
      }
      const payload = source.slice(source.indexOf(',') + 1);
      if(payload.length === 0 || payload.length%4 !== 0 || !/^[A-Za-z0-9+/]*={0,2}$/.test(payload)) {
        throw new Error(`${name}.${key} contains invalid base64 texture data.`);
      }
    }
    result[ ((key) as keyof MaterialTextures)] = source;
  }
  return result;
}

/** Count original image-file bytes without allocating a decoded copy. @param source */
function textureByteLength(source: string) {
  const payloadLength = source.length - source.indexOf(',') - 1;
  const padding = source.endsWith('==') ? 2 : source.endsWith('=') ? 1 : 0;
  return Math.floor(payloadLength*3/4) - padding;
}

/** @param value @param length @param label */
function readTuple(value: unknown, length: number, label: string) {
  if(!Array.isArray(value) || value.length !== length) {
    throw new Error(`${label} must contain exactly ${length} numbers.`);
  }
  return value.map((component) => readUnitNumber(component, label));
}

/** @param value @param label */
function readUnitNumber(value: unknown, label: string) {
  if(typeof value !== 'number' || !Number.isFinite(value) || value < 0 || value > 1) {
    throw new Error(`${label} must be a finite number from 0 to 1.`);
  }
  return value;
}
