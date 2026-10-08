// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { PARTICLE_RENDERER } from './constants.ts';
import { GPU_ADDRESS_MODE, GPU_FILTER } from '../rendering/gpu/constants.ts';
import type { GpuTriangleRenderer } from '../rendering/renderers/gpu-triangle-renderer.ts';
import type { CompiledParticleEffect } from './asset.ts';
import type { Device } from '@luma.gl/core';
import { Texture } from '@luma.gl/core';
import { readParticleTextureSource } from './texture-source.ts';
import wgsl from '../../shaders/particles/textured.wgsl?raw';
import vertex from '../../shaders/particles/textured.vert.glsl?raw';
import fragment from '../../shaders/particles/textured.frag.glsl?raw';

/** Perspective texture shaders with mat4 viewProjection + vec4 tint uniforms.
 * Compatible with GpuTriangleRenderer post processing. UV origin is top-left;
 * sampled sRGB image colors become linear before multiplying HDR particle tint.
 */
export const PARTICLE_TEXTURE_SHADERS = Object.freeze({ wgsl, glsl: { vertex, fragment } });

/** Owns decoded particle textures for one renderer. The device is borrowed.
 * Call prepare when the compiled definition changes; await it before drawing.
 * Concurrent prepares discard obsolete work. Destroy before the GPU renderer.
 */
export class ParticleTextureSet {
  declare device: Device;
  declare textures: Map<string, Texture>;
  declare activeSources: Set<string>;
  declare generation: number;
  declare disposed: boolean;

  /** @param renderer */
  constructor(renderer: GpuTriangleRenderer) {
    this.device = renderer.device;

    this.textures = new Map();

    this.activeSources = new Set();
    this.generation = 0;
    this.disposed = false;
  }

  /** Atomically replace the texture set, retaining unchanged GPU allocations.
   * Returns false if a newer prepare or destruction superseded this request.
   * @param definition
   */
  async prepare(definition: CompiledParticleEffect): Promise<boolean> {
    const generation = ++this.generation;
    const sources = new Set(definition.emitters.flatMap(emitter =>
      (emitter.renderer === PARTICLE_RENDERER.TEXTURED || emitter.renderer === PARTICLE_RENDERER.FLIPBOOK) && emitter.texture
        ? [emitter.texture.source] : [],
    ));

    const next: Map<string, Texture> = new Map();

    const created: Texture[] = [];
    try {
      for(const source of sources) {
        const existing = this.textures.get(source);
        if(existing) {
          next.set(source, existing);
          continue;
        }
        readParticleTextureSource(source);
        const encoded = source.slice(source.indexOf(',') + 1);
        const bytes = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
        const bitmap = await createImageBitmap(new Blob([bytes], { type: 'image/png' }), {
          premultiplyAlpha: 'none', colorSpaceConversion: 'none',
        });
        try {
          if(this.disposed || generation !== this.generation) {
            return false;
          }
          const texture = this.device.createTexture({
            id: 'particle-image', data: bitmap, width: bitmap.width, height: bitmap.height,
            format: 'rgba8unorm-srgb', usage: Texture.SAMPLE | Texture.COPY_DST | Texture.RENDER_ATTACHMENT,
            sampler: { minFilter: GPU_FILTER.LINEAR, magFilter: GPU_FILTER.LINEAR, addressModeU: GPU_ADDRESS_MODE.CLAMP_TO_EDGE, addressModeV: GPU_ADDRESS_MODE.CLAMP_TO_EDGE },
          });
          created.push(texture);
          next.set(source, texture);
        } finally {
          bitmap.close();
        }
      }
      if(this.disposed || generation !== this.generation) {
        return false;
      }
      // Keep the displayed definition drawable until its owner activates the prepared replacement.
      for(const [source, texture] of this.textures) {
        if(this.activeSources.has(source)) {
          next.set(source, texture);
        } else if(!next.has(source)) {
          texture.destroy();
        }
      }
      this.textures = next;
      created.length = 0;
      return true;
    } finally {
      for(const texture of created) {
        texture.destroy();
      }
    }
  }

  /** Commit the prepared definition; release images no longer drawn.
   * @param definition
   */
  activate(definition: CompiledParticleEffect | null) {
    this.activeSources = new Set(definition?.emitters.flatMap(emitter =>
      (emitter.renderer === PARTICLE_RENDERER.TEXTURED || emitter.renderer === PARTICLE_RENDERER.FLIPBOOK) && emitter.texture
        ? [emitter.texture.source] : [],
    ) ?? []);
    for(const [source, texture] of this.textures) {
      if(!this.activeSources.has(source)) {
        texture.destroy();
        this.textures.delete(source);
      }
    }
  }

  /** Borrow a texture after a successful prepare. @param source */
  get(source: string) {
    const texture = this.textures.get(source);
    if(!texture) {
      throw new Error('Particle image is not ready; await ParticleTextureSet.prepare before rendering.');
    }
    return texture;
  }

  /** Release owned GPU textures and invalidate any decode still in flight. */
  destroy() {
    this.disposed = true;
    this.generation += 1;
    for(const texture of this.textures.values()) {
      texture.destroy();
    }
    this.textures.clear();
  }
}
