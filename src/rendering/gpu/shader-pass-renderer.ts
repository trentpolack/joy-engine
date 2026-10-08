// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { GPU_ADDRESS_MODE, GPU_FILTER } from './constants.ts';
import type { Device, Sampler } from '@luma.gl/core';
import type { ShaderPassRendererProps } from '@luma.gl/engine';
import { ShaderPassRenderer as LumaShaderPassRenderer } from '@luma.gl/engine';

/**
 * luma.gl pass execution with linear sampling for the shared color chain.
 * luma.gl 9.4 allocates its swap textures with nearest sampling; FXAA and
 * fractional blur offsets require interpolation. Reapply after resize because
 * upstream replaces these attachments. All other execution stays upstream.
 */
export class ShaderPassRenderer extends LumaShaderPassRenderer {
  declare colorSampler: Sampler;

  /**
   * @param device Borrowed device.
   * @param props Pass descriptions and color format.
   */
  constructor(device: Device, props: ShaderPassRendererProps) {
    super(device, props);
    // Textures borrow one shared sampler; it belongs to this renderer rather
    // than being reallocated (and orphaned) whenever resize runs.
    this.colorSampler = device.createSampler({ minFilter: GPU_FILTER.LINEAR, magFilter: GPU_FILTER.LINEAR, addressModeU: GPU_ADDRESS_MODE.CLAMP_TO_EDGE, addressModeV: GPU_ADDRESS_MODE.CLAMP_TO_EDGE });
    this.configureColorSampling();
  }

  /** @param [size] Drawing-buffer dimensions. */
  resize(size?: [
    number,
    number
]) {
    super.resize(size);
    this.configureColorSampling();
  }

  /** Release pass resources before their borrowed sampler. */
  destroy() {
    super.destroy();
    this.colorSampler.destroy();
  }

  /** @private */
  private configureColorSampling() {
    for(const framebuffer of [this.swapFramebuffers.current, this.swapFramebuffers.next]) {
      for(const attachment of framebuffer.colorAttachments) {
        attachment.texture.setSampler(this.colorSampler);
      }
    }
  }
}
