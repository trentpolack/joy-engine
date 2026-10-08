// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { GPU_ADDRESS_MODE, GPU_FILTER, GPU_TEXTURE_FORMAT } from '../gpu/constants.ts';
import type { Framebuffer, Device } from '@luma.gl/core';
import type { PostProcessingConfig } from './config.ts';
import { Texture } from '@luma.gl/core';
import { ShaderPassRenderer } from '../gpu/shader-pass-renderer.ts';
import { createEffectPipeline, createEffectUniforms, effectPipelineKey } from './effect-pipeline.ts';

/**
 * Owns scene attachments and luma.gl's ordered HDR-to-display effects.
 * The device is borrowed. Upstream ShaderPassRenderer owns filter targets,
 * shader assembly, uniform packing, bloom reconstruction and presentation.
 */
export class Postprocessor {
  declare device: Device;
  declare format: typeof GPU_TEXTURE_FORMAT.RGBA8UNORM | typeof GPU_TEXTURE_FORMAT.RGBA16FLOAT;
  declare targets: { texture: Texture; depth: Texture; framebuffer: Framebuffer; }[];
  declare effects: ShaderPassRenderer | null;
  declare pipelineKey: string;
  declare width: number;
  declare height: number;

  /** @param device */
  constructor(device: Device) {
    this.device = device;
    const caps = device.getTextureFormatCapabilities(GPU_TEXTURE_FORMAT.RGBA16FLOAT);

    this.format = caps.create && caps.render && caps.filter && caps.blend ? GPU_TEXTURE_FORMAT.RGBA16FLOAT : GPU_TEXTURE_FORMAT.RGBA8UNORM;

    this.targets = [];

    this.effects = null;
    this.pipelineKey = '';
    this.width = 0;
    this.height = 0;
  }

  /**
   * Resize owned scene attachments. Filter targets follow drawing-buffer size,
   * and viewport-relative radii preserve the same framing at different DPI.
   * @param width Backing width.
   * @param height Backing height.
   * @param _cssWidth Logical width.
   * @param _cssHeight Logical height.
   */
  resize(width: number, height: number, _cssWidth: number, _cssHeight: number) {
    if(this.width === width && this.height === height) {
      return;
    }
    const replacement = this.createTarget(width, height);
    try {
      this.effects?.resize([width, height]);
      this.effects?.resetHistory();
    } catch(error) {
      destroyTarget(replacement);
      throw error;
    }
    this.destroyTargets();
    this.targets = [replacement];
    this.width = width;
    this.height = height;
  }

  /**
   * Allocate a complete scene destination or release every partial resource.
   * @param width
   * @param height
   */
  createTarget(width: number, height: number): {
    texture: Texture;
    depth: Texture;
    framebuffer: Framebuffer;
} {

    let texture: Texture | null = null;

    let depth: Texture | null = null;
    try {
      // Scene render target.
      texture = this.device.createTexture({
        id: 'postprocessor-scene-color', width, height, format: this.format,
        usage: Texture.RENDER | Texture.SAMPLE,
        sampler: { minFilter: GPU_FILTER.LINEAR, magFilter: GPU_FILTER.LINEAR, addressModeU: GPU_ADDRESS_MODE.CLAMP_TO_EDGE, addressModeV: GPU_ADDRESS_MODE.CLAMP_TO_EDGE }
      });

      // Depth buffer render target.
      depth = this.device.createTexture({
        id: 'postprocessor-scene-depth', width, height, format: GPU_TEXTURE_FORMAT.DEPTH24PLUS,
        usage: Texture.RENDER | Texture.SAMPLE,
        sampler: { minFilter: GPU_FILTER.NEAREST, magFilter: GPU_FILTER.NEAREST }
      });

      // Create the framebuffer.
      const framebuffer = this.device.createFramebuffer({ width, height, colorAttachments: [texture], depthStencilAttachment: depth });
      return { texture, depth, framebuffer };
    } catch(error) {
      depth?.destroy();
      texture?.destroy();

      throw error;
    }
  }

  /** 
   * Borrowed scene destination; valid until resize or destruction.
   */
  get sceneFramebuffer() {
    return this.targets[0].framebuffer;
  }

  /**
   * Resolve linear scene color to the default canvas. The caller submits GPU work.
   * @param config
   * @param timeSeconds Presentation clock in seconds.
   * @param [sourceTexture] Optional preprocessed linear scene, borrowed for this submission.
   */
  render(config: PostProcessingConfig, timeSeconds: number, sourceTexture: Texture = this.targets[0].texture) {
    const shortestAxis = Math.min(this.width, this.height);
    const key = effectPipelineKey(config, shortestAxis);
    if(!this.effects || this.pipelineKey !== key) {
      // Publish only a fully constructed replacement; the previous graph stays
      // usable if upstream rejects an unsupported shader or target format.

      let replacement: ShaderPassRenderer | null = null;
      try {
        replacement = new ShaderPassRenderer(this.device, {
          shaderPasses: createEffectPipeline(config, shortestAxis, this.format),
          colorFormat: this.format
        });

        replacement.resize([this.width, this.height]);
      } catch(error) {
        replacement?.destroy();
        throw error;
      }

      this.effects?.destroy();
      this.effects = replacement;
      this.pipelineKey = key;
    }

    this.effects.renderToScreen({ 
      sourceTexture,
      uniforms: createEffectUniforms(config, shortestAxis, timeSeconds)
    });
  }

  /** Release attachments before resizing or destroying the owner. */
  destroyTargets() {
    for(const target of this.targets) {
      destroyTarget(target);
    }
    this.targets = [];
  }

  /** Release all owned resources; the borrowed device remains alive. */
  destroy() {
    this.effects?.destroy();
    this.effects = null;
    this.pipelineKey = '';
    this.destroyTargets();
  }
}

/** @param target */
function destroyTarget(target: {
    texture: Texture;
    depth: Texture;
    framebuffer: Framebuffer;
}) {
  target.framebuffer.destroy();
  target.depth.destroy();
  target.texture.destroy();
}
