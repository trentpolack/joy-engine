// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { GPU_ADDRESS_MODE, GPU_FILTER, GPU_TEXTURE_FORMAT } from '../gpu/constants.ts';
import { MATERIAL_ALPHA_MODE } from './constants.ts';
import type { PbrMaterial } from '../../core/types.ts';
import type { Device } from '@luma.gl/core';
import { Buffer, Texture } from '@luma.gl/core';

export type { PbrMaterial };
export interface PbrMaterialBindings {
  uniforms: Buffer;
  baseColorTexture: Texture;
  normalTexture: Texture;
  emissiveTexture: Texture;
  metallicTexture: Texture;
  roughnessTexture: Texture;
  aoTexture: Texture;
}

/** Owns material uniforms and shared image allocations for one borrowed GPU device.
 * Factors use linear RGB; image RGB is stored unchanged for shader sRGB decoding.
 * Normal maps use tangent-space XYZ; scalar maps sample their red channel. Image
 * coordinates use a top-left origin and repeat wrapping. Destroy before the device.
 */
export class PbrMaterialSet {
  declare device: Device;
  private declare textures: Map<string, Texture>;
  private declare currentBindings: PbrMaterialBindings[];
  private declare pendingLoad: AbortController | null;
  declare generation: number;
  declare disposed: boolean;
  declare whiteTexture: Texture;
  declare normalTexture: Texture;

  /** @param device Borrowed; never destroyed here. */
  constructor(device: Device) {
    this.device = device;
    /** @private */
    this.textures = new Map();
    /** @private */
    this.currentBindings = [];
    /** @private */
    this.pendingLoad = null;
    this.generation = 0;
    this.disposed = false;

    this.whiteTexture = createFallbackTexture(device, 'pbr-white', [255, 255, 255, 255]);
    try {
      this.normalTexture = createFallbackTexture(device, 'pbr-flat-normal', [128, 128, 255, 255]);
    } catch(error) {
      this.whiteTexture.destroy();
      throw error;
    }
  }

  /** Current borrowed draw bindings, in authored material order. Treat as read-only.
   */
  get bindings() {
    return this.currentBindings;
  }

  /** Atomically replace materials, reusing images with unchanged source strings.
   * Snapshots authored factors before loading. Failed decoding/allocation rejects
   * and retains the previous drawable set. Returns false when superseded/disposed;
   * superseded requests abort network work and release any images still decoding.
   * @param materials Authored data; never mutated.
   * @param [commit] Optional synchronous owner commit after allocation,
   * before swapping bindings. Return false to cancel. Throwing retains old bindings;
   * prepare fallible owner work before this callback and never mutate then throw.
   * @returns Whether these bindings became the active set.
   */
  async update(materials: readonly PbrMaterial[], commit?: () => boolean): Promise<boolean> {
    if(this.disposed) {
      return false;
    }
    const snapshots = materials.map(material => {
      validateMaterial(material);
      return { uniforms: packMaterialUniforms(material), textures: { ...material.textures } };
    });
    const generation = ++this.generation;
    this.pendingLoad?.abort();
    const controller = new AbortController();
    this.pendingLoad = controller;
    const sources = new Set(snapshots.flatMap(material =>
      Object.values(material.textures).filter(source => source !== null),
    ));

    const nextTextures: Map<string, Texture> = new Map();

    const createdTextures: Texture[] = [];

    const nextBindings: PbrMaterialBindings[] = [];
    try {
      for(const source of sources) {
        const existing = this.textures.get(source);
        if(existing) {
          nextTextures.set(source, existing);
          continue;
        }
        const image = await decodeImage(source, controller.signal);
        try {
          if(this.disposed || generation !== this.generation) {
            return false;
          }
          const limit = this.device.limits.maxTextureDimension2D;
          if(image.width < 1 || image.height < 1 || image.width > limit || image.height > limit) {
            throw new Error(`PBR texture dimensions must be 1–${limit} pixels per axis.`);
          }
          const texture = this.device.createTexture({
            id: 'pbr-image', data: image, width: image.width, height: image.height,
            format: GPU_TEXTURE_FORMAT.RGBA8UNORM, usage: Texture.SAMPLE | Texture.COPY_DST | Texture.RENDER_ATTACHMENT,
            sampler: { minFilter: GPU_FILTER.LINEAR, magFilter: GPU_FILTER.LINEAR, addressModeU: GPU_ADDRESS_MODE.REPEAT, addressModeV: GPU_ADDRESS_MODE.REPEAT }
          });
          createdTextures.push(texture);
          nextTextures.set(source, texture);
        } finally {
          image.close();
        }
      }
      if(this.disposed || generation !== this.generation) {
        return false;
      }
      for(const material of snapshots) {
        const uniforms = this.device.createBuffer({
          id: 'pbr-material-uniforms', data: material.uniforms, usage: Buffer.UNIFORM | Buffer.COPY_DST
        });
        const textures = material.textures;
        nextBindings.push({
          uniforms,
          baseColorTexture: textureOrFallback(nextTextures, textures.baseColor, this.whiteTexture),
          normalTexture: textureOrFallback(nextTextures, textures.normal, this.normalTexture),
          emissiveTexture: textureOrFallback(nextTextures, textures.emissive, this.whiteTexture),
          metallicTexture: textureOrFallback(nextTextures, textures.metallic, this.whiteTexture),
          roughnessTexture: textureOrFallback(nextTextures, textures.roughness, this.whiteTexture),
          aoTexture: textureOrFallback(nextTextures, textures.ao, this.whiteTexture)
        });
      }

      if(commit && !commit()) {
        return false;
      }
      const previousBindings = this.currentBindings;
      const previousTextures = this.textures;
      this.currentBindings = nextBindings;
      this.textures = nextTextures;
      createdTextures.length = 0;
      for(const binding of previousBindings) {
        binding.uniforms.destroy();
      }
      for(const [source, texture] of previousTextures) {
        if(!nextTextures.has(source)) {
          texture.destroy();
        }
      }
      return true;
    } catch(error) {
      if(this.disposed || generation !== this.generation) {
        return false;
      }
      throw error;
    } finally {
      for(const texture of createdTextures) {
        texture.destroy();
      }
      if(this.currentBindings !== nextBindings) {
        for(const binding of nextBindings) {
          binding.uniforms.destroy();
        }
      }
      if(this.pendingLoad === controller) {
        this.pendingLoad = null;
      }
    }
  }

  /** Release owned GPU allocations and cancel pending loads. Safe to call twice. */
  destroy() {
    if(this.disposed) {
      return;
    }
    this.disposed = true;
    this.generation+= 1;
    this.pendingLoad?.abort();
    this.pendingLoad = null;
    for(const binding of this.currentBindings) {
      binding.uniforms.destroy();
    }
    this.currentBindings = [];
    for(const texture of this.textures.values()) {
      texture.destroy();
    }
    this.textures.clear();
    this.whiteTexture.destroy();
    this.normalTexture.destroy();
  }
}

/** Shader layout: baseColor, emissiveMetallic, roughnessAlpha (three vec4 values).
 * roughnessAlpha stores roughness, alpha mode, cutoff and whether a normal map exists.
 * Authored factors multiply texture samples; assigning a texture never resets them.
 * @param material
 */
function packMaterialUniforms(material: PbrMaterial) {
  const alphaMode = material.alphaMode === MATERIAL_ALPHA_MODE.BLEND ? 2 : material.alphaMode === MATERIAL_ALPHA_MODE.MASK ? 1 : 0;
  return new Float32Array([
    material.baseColor[0], material.baseColor[1], material.baseColor[2], material.baseColor[3],
    material.emissive[0], material.emissive[1], material.emissive[2], material.metallic,
    material.roughness, alphaMode, material.alphaCutoff, material.textures.normal === null ? 0 : 1
  ]);
}

/** Validate the public material boundary before decoding or allocating resources.
 * HDR RGB factors stay unbounded above while alpha and scalar factors use 0–1.
 * @param material
 */
function validateMaterial(material: PbrMaterial) {
  for(const [channels, count] of  (([
    [material.baseColor, 4], [material.emissive, 3]
  ]) as [
    readonly number[],
    number
][])) {
    if(channels.length !== count || channels.some(value => !Number.isFinite(Math.fround(value)) || value < 0)) {
      throw new Error('PBR material colors require nonnegative finite float32 channels (RGBA base color and RGB emissive).');
    }
  }
  for(const value of [material.baseColor[3], material.metallic, material.roughness, material.alphaCutoff]) {
    if(!Number.isFinite(value) || value < 0 || value > 1) {
      throw new Error('PBR material alpha, metallic, roughness and alpha cutoff must be between 0 and 1.');
    }
  }
  if(!([MATERIAL_ALPHA_MODE.OPAQUE, MATERIAL_ALPHA_MODE.MASK, MATERIAL_ALPHA_MODE.BLEND] as readonly string[]).includes(material.alphaMode)) {
    throw new Error('PBR material alpha mode must be OPAQUE, MASK or BLEND.');
  }
  for(const channel of  ((['baseColor', 'normal', 'emissive', 'metallic', 'roughness', 'ao']) as (keyof PbrMaterial['textures'])[])) {
    const source = material.textures[channel];
    if(source !== null && (typeof source !== 'string' || !source.trim())) {
      throw new Error('PBR material texture sources must be nonempty URLs or null.');
    }
  }
}

/** @param source @param signal */
async function decodeImage(source: string, signal: AbortSignal): Promise<ImageBitmap> {
  const response = await fetch(source, { signal });
  if(!response.ok) {
    throw new Error(`PBR texture request failed (${response.status}).`);
  }
  const blob = await response.blob();
  signal.throwIfAborted();
  return createImageBitmap(blob, { premultiplyAlpha: 'none', colorSpaceConversion: 'none' });
}

/** @param device @param id @param color */
function createFallbackTexture(device: Device, id: string, color: number[]) {
  return device.createTexture({
    id, width: 1, height: 1, data: new Uint8Array(color), format: GPU_TEXTURE_FORMAT.RGBA8UNORM,
    usage: Texture.SAMPLE | Texture.COPY_DST,
    sampler: { minFilter: GPU_FILTER.LINEAR, magFilter: GPU_FILTER.LINEAR, addressModeU: GPU_ADDRESS_MODE.REPEAT, addressModeV: GPU_ADDRESS_MODE.REPEAT }
  });
}

/** @param textures
 * @param source @param fallback
 */
function textureOrFallback(textures: Map<string, Texture>, source: string | null, fallback: Texture) {
  return source === null ? fallback : textures.get(source) ?? fallback;
}
