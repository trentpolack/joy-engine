// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { GPU_BACKEND, GPU_COMPARE, GPU_TEXTURE_FORMAT, GPU_TOPOLOGY, GPU_VERTEX_FORMAT, SHADER_ENTRY_POINT } from '../gpu/constants.ts';
import type { Texture, TextureFormatColor, RenderPass, Device, Shader, RenderPipeline, VertexArray } from '@luma.gl/core';
import type { TriangleShaderSources } from './gpu-triangle-renderer.ts';
import { Buffer } from '@luma.gl/core';
import { DynamicGpuBuffer } from '../gpu/dynamic-gpu-buffer.ts';
import { createPortableShaders } from '../gpu/gpu-device.ts';

export interface TransparentDraw {
  vertices: number[] | Float32Array;
  texture?: Texture;
}

/** Optional textured material owned by GpuTriangleRenderer. Borrows its device,
 * scene uniform buffer and render pass; never clears or resolves the scene.
 */
export class TexturedTriangles {
  declare device: Device;
  declare shaders: ReturnType<typeof createPortableShaders>;
  declare pipeline: RenderPipeline;
  declare vertexArray: VertexArray;
  declare vertices: DynamicGpuBuffer;

  /** @param device
   * @param sources
   * @param colorFormat
   */
  constructor(device: Device, sources: TriangleShaderSources, colorFormat: TextureFormatColor | undefined) {
    this.device = device;
    this.shaders = createPortableShaders(device, sources, 'textured-triangles');
    this.pipeline = device.createRenderPipeline({
      id: 'textured-triangles', vs: this.shaders.vertex, fs: this.shaders.fragment,
      vertexEntryPoint: SHADER_ENTRY_POINT.VERTEX, fragmentEntryPoint: SHADER_ENTRY_POINT.FRAGMENT, topology: GPU_TOPOLOGY.TRIANGLE_LIST,
      bufferLayout: [{ name: 'vertices', byteStride: 36, attributes: [
        { attribute: 'position', format: GPU_VERTEX_FORMAT.FLOAT32X3, byteOffset: 0 },
        { attribute: 'color', format: GPU_VERTEX_FORMAT.FLOAT32X4, byteOffset: 12 },
        { attribute: 'uv', format: GPU_VERTEX_FORMAT.FLOAT32X2, byteOffset: 28 },
      ] }],
      depthStencilAttachmentFormat: GPU_TEXTURE_FORMAT.DEPTH24PLUS,
      ...(colorFormat ? { colorAttachmentFormats: [colorFormat] } : {}),
      parameters: {
        depthWriteEnabled: false, depthCompare: GPU_COMPARE.LESS_EQUAL, blend: true,
        blendColorOperation: 'add', blendAlphaOperation: 'add',
        blendColorSrcFactor: 'src-alpha', blendColorDstFactor: 'one-minus-src-alpha',
        blendAlphaSrcFactor: 'one', blendAlphaDstFactor: 'one-minus-src-alpha',
      },
    });
    this.vertexArray = device.createVertexArray({
      shaderLayout: this.pipeline.shaderLayout, bufferLayout: this.pipeline.bufferLayout,
    });
    this.vertices = new DynamicGpuBuffer(device, {
      id: 'textured-triangle-vertices', usage: Buffer.VERTEX | Buffer.COPY_DST,
      initialByteLength: 64 * 1024,
      onReplace: buffer => {
        this.vertexArray.setBuffer(0, buffer);
        if(device.type === GPU_BACKEND.WEBGL) {
          this.vertexArray.setBuffer(1, buffer);
          this.vertexArray.setBuffer(2, buffer);
        }
      },
    });
  }

  /** Upload once before beginning a pass: WebGPU writes precede queued draws.
   * @param draws
   */
  prepare(draws: readonly TransparentDraw[]) {
    const length = draws.reduce((total, draw) => total + (draw.texture ? draw.vertices.length : 0), 0);
    if(length === 0) {
      return;
    }
    const data = new Float32Array(length);
    let offset = 0;
    for(const draw of draws) {
      if(draw.texture) {
        data.set(draw.vertices, offset);
        offset += draw.vertices.length;
      }
    }
    this.vertices.write(data);
  }

  /** @param pass
   * @param uniforms
   * @param texture
   * @param firstVertex @param vertexCount
   */
  draw(pass: RenderPass, uniforms: Buffer, texture: Texture, firstVertex: number, vertexCount: number) {
    pass.setPipeline(this.pipeline);
    pass.setBindings({ uniforms, particleTexture: texture });
    pass.setVertexArray(this.vertexArray);
    pass.draw({ firstVertex, vertexCount });
  }

  destroy() {
    this.vertices.destroy();
    this.vertexArray.destroy();
    this.pipeline.destroy();
    for(const shader of new Set(Object.values(this.shaders))) {
      shader.destroy();
    }
  }
}
