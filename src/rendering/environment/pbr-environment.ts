// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {Texture} from '@luma.gl/core';
import type {Device} from '@luma.gl/core';
import {PBREnvironmentGenerator} from '@luma.gl/experimental';
import type {PreparedPBREnvironment, PreparePBREnvironmentOptions} from '@luma.gl/experimental';

/** Original equirectangular pixels: byte RGB is linear UNORM; float RGB is linear HDR. */
export interface PbrEnvironmentPixels {
  width: number;
  height: number;
  data: Uint8Array | Float32Array;
}

export type PbrEnvironmentOptions = Omit<PreparePBREnvironmentOptions, 'source' | 'intensity' | 'rotation'>;

/**
 * Owns luma-generated irradiance, filtered reflection, and BRDF textures.
 * Source textures and the device are borrowed. Prepare submits integration work;
 * call during setup or environment replacement, never from ordinary frame rendering.
 */
export class PbrEnvironment {
  private readonly device: Device;
  private ownedSource: Texture | null = null;
  private readonly generator: PBREnvironmentGenerator;
  private current: PreparedPBREnvironment | null = null;
  private disposed = false;
  /** Linear radiance multiplier; changing it does not regenerate textures. */
  intensity = 1;
  /** Horizontal environment rotation in radians. */
  rotation = 0;

  constructor(device: Device) {
    this.device = device;
    this.generator = new PBREnvironmentGenerator(device);
  }

  /** Current borrowed texture bindings; null before preparation or after destruction. */
  get bindings() {
    return this.current;
  }

  /** Atomically replace a prepared environment. Failure retains the accepted textures. */
  prepare(source: Texture, options: PbrEnvironmentOptions = {}) {
    if(this.disposed) {
      throw new Error('PbrEnvironment is destroyed.');
    }
    const replacement = this.generator.prepare({source, ...options});
    const previous = this.current;
    this.current = replacement;
    previous?.destroy();
    this.ownedSource?.destroy();
    this.ownedSource = null;
  }

  /** Prepare original pixels, owning their uploaded source texture as well as generated maps. */
  preparePixels(pixels: PbrEnvironmentPixels, options: PbrEnvironmentOptions = {}) {
    if(!Number.isInteger(pixels.width) || !Number.isInteger(pixels.height) || pixels.width < 1 || pixels.height < 1 ||
      pixels.width > this.device.limits.maxTextureDimension2D || pixels.height > this.device.limits.maxTextureDimension2D ||
      pixels.data.length !== pixels.width*pixels.height*4) {
      throw new RangeError('Environment pixels need supported dimensions and four channels per pixel.');
    }
    let data: Uint8Array | Uint16Array = pixels.data instanceof Uint8Array ? pixels.data : new Uint16Array(pixels.data.length);
    if(pixels.data instanceof Float32Array) {
      for(let index = 0; index < pixels.data.length; index++) {
        const value = pixels.data[index];
        if(!Number.isFinite(value) || value < 0 || value > 65504) {
          throw new RangeError('HDR environment pixels must be finite, nonnegative half-float values.');
        }
        data[index] = encodePositiveHalf(value);
      }
    }
    const source = this.device.createTexture({id: 'pbr-environment-source', width: pixels.width, height: pixels.height,
      data, format: pixels.data instanceof Uint8Array ? 'rgba8unorm' : 'rgba16float', usage: Texture.SAMPLE | Texture.COPY_DST,
      sampler: {minFilter: 'linear', magFilter: 'linear', addressModeU: 'repeat', addressModeV: 'clamp-to-edge'}});
    try {
      this.prepare(source, options);
      this.ownedSource = source;
    } catch(error) {
      source.destroy();
      throw error;
    }
  }

  /** Release generated textures and integration resources, retaining the borrowed source. */
  destroy() {
    if(this.disposed) {
      return;
    }
    this.disposed = true;
    this.current?.destroy();
    this.current = null;
    this.ownedSource?.destroy();
    this.ownedSource = null;
    this.generator.destroy();
  }
}

/** Convert validated nonnegative radiance to IEEE half-float, including subnormals and mantissa carry. */
function encodePositiveHalf(value: number): number {
  if(value < 2**(-14)) {
    return Math.round(value*2**24);
  }
  const exponent = Math.floor(Math.log2(value));
  return (exponent + 15)*1024 + Math.round((value/(2**exponent) - 1)*1024);
}
