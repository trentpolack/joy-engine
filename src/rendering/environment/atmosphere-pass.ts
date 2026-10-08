// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { GPU_COMPARE, GPU_TEXTURE_FORMAT, GPU_TOPOLOGY, SHADER_ENTRY_POINT } from '../gpu/constants.ts';
import type { Device, TextureFormatColor, RenderPass, Shader, RenderPipeline, VertexArray } from '@luma.gl/core';
import { Buffer } from '@luma.gl/core';
import { createPortableShaders } from '../gpu/gpu-device.ts';
import { ATMOSPHERE_SHADERS, ATMOSPHERE_RENDER_UNIFORM_BYTES } from './atmosphere-rendering.ts';

/**
 * Owns a procedural background pipeline; borrows the device and render pass.
 * Draws into the scene's linear color target without changing scene depth.
 */
export class AtmospherePass {
  private declare shaders: ReturnType<typeof createPortableShaders> | null;
  private declare pipeline: RenderPipeline | null;
  private declare vertexArray: VertexArray | null;
  private declare uniformBuffer: Buffer | null;
  declare destroyed: boolean;

  /**
   * @param device Borrowed device.
   * @param [colorFormat] Scene target format; omitted for direct canvas rendering.
   */
  constructor(device: Device, colorFormat?: TextureFormatColor) {
    /** @private */
    this.shaders = null;
    /** @private */
    this.pipeline = null;
    /** @private */
    this.vertexArray = null;
    /** @private */
    this.uniformBuffer = null;
    this.destroyed = false;

    try {
      this.shaders = createPortableShaders(device, ATMOSPHERE_SHADERS, 'atmosphere');
      this.pipeline = device.createRenderPipeline({
        id: 'atmosphere-background',
        vs: this.shaders.vertex,
        fs: this.shaders.fragment,
        vertexEntryPoint: SHADER_ENTRY_POINT.VERTEX,
        fragmentEntryPoint: SHADER_ENTRY_POINT.FRAGMENT,
        topology: GPU_TOPOLOGY.TRIANGLE_LIST,
        bufferLayout: [],
        depthStencilAttachmentFormat: GPU_TEXTURE_FORMAT.DEPTH24PLUS,
        ...(colorFormat ? {colorAttachmentFormats: [colorFormat]} : {}),
        parameters: {depthWriteEnabled: false, depthCompare: GPU_COMPARE.ALWAYS, blend: false}
      });
      this.vertexArray = device.createVertexArray({
        id: 'atmosphere-empty-vertices',
        shaderLayout: this.pipeline.shaderLayout,
        bufferLayout: []
      });
      this.uniformBuffer = device.createBuffer({
        id: 'atmosphere-uniforms',
        byteLength: ATMOSPHERE_RENDER_UNIFORM_BYTES,
        usage: Buffer.UNIFORM | Buffer.COPY_DST
      });
    } catch(error) {
      this.destroy();
      throw error;
    }
  }

  /**
   * Submit the background before opaque geometry, using caller-owned packed uniforms.
   * @param pass Borrowed open scene pass.
   * @param uniforms Camera/environment snapshot.
   */
  draw(pass: RenderPass, uniforms: Float32Array<ArrayBuffer>) {
    if(this.destroyed || !this.pipeline || !this.vertexArray || !this.uniformBuffer) {
      throw new Error('AtmospherePass is destroyed.');
    }
    if(uniforms.byteLength !== ATMOSPHERE_RENDER_UNIFORM_BYTES) {
      throw new RangeError('AtmospherePass needs a complete atmosphere uniform block.');
    }
    this.uniformBuffer.write(uniforms);
    pass.setPipeline(this.pipeline);
    pass.setBindings({uniforms: this.uniformBuffer});
    pass.setVertexArray(this.vertexArray);
    pass.draw({vertexCount: 3});
  }

  /**
   * Release owned GPU resources once, including after incomplete construction.
   */
  destroy() {
    if(this.destroyed) {
      return;
    }
    this.destroyed = true;
    this.vertexArray?.destroy();
    this.uniformBuffer?.destroy();
    this.pipeline?.destroy();
    if(this.shaders) {
      for(const shader of new Set(Object.values(this.shaders))) {
        shader.destroy();
      }
    }
  }
}
