// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {Buffer} from '@luma.gl/core';
import type {Device, RenderPass, RenderPipeline, VertexArray} from '@luma.gl/core';
import type {ShadowRenderView} from '@luma.gl/experimental';
import {DynamicGpuBuffer} from '../gpu/dynamic-gpu-buffer.ts';
import {createPortableShaders} from '../gpu/gpu-device.ts';
import type {ShadowOptions} from './types.ts';
import {DEFAULT_SHADOW_OPTIONS} from './types.ts';
import wgsl from '../../../shaders/rendering/shadows/shadow-caster.wgsl?raw';

/**
 * Owns colored caster geometry and per-recorded-view matrix snapshots.
 * PBR casters borrow those snapshots to share a light view without rewriting it.
 */
export class ShadowCasters {
  private readonly device: Device;
  private readonly matrices: Buffer[] = [];
  private readonly shader: ReturnType<typeof createPortableShaders>;
  private readonly pipeline: RenderPipeline;
  private readonly vertexArray: VertexArray;
  private readonly vertices: DynamicGpuBuffer;
  private cursor = 0;
  private vertexCount = 0;
  private disposed = false;

  constructor(device: Device, options: ShadowOptions = {}) {
    this.device = device;
    const allocated: {destroy(): void}[] = [];
    try {
      this.shader = createPortableShaders(device, {wgsl, glsl: {vertex: '', fragment: ''}}, 'shadow-caster');
      allocated.push(this.shader.vertex);
      this.pipeline = device.createRenderPipeline({
        id: 'colored-shadow-caster', vs: this.shader.vertex, fs: this.shader.fragment,
        vertexEntryPoint: 'vertexMain', fragmentEntryPoint: 'fragmentMain',
        topology: 'triangle-list', colorAttachmentFormats: [], depthStencilAttachmentFormat: 'depth32float',
        bufferLayout: [{name: 'vertices', byteStride: 28, attributes: [{attribute: 'position', format: 'float32x3', byteOffset: 0}]}],
        parameters: {depthWriteEnabled: true, depthCompare: 'less-equal', cullMode: 'none',
          depthBias: options.depthBias ?? DEFAULT_SHADOW_OPTIONS.depthBias,
          depthBiasSlopeScale: options.depthBiasSlopeScale ?? DEFAULT_SHADOW_OPTIONS.depthBiasSlopeScale}
      });
      allocated.push(this.pipeline);
      this.vertexArray = device.createVertexArray({shaderLayout: this.pipeline.shaderLayout, bufferLayout: this.pipeline.bufferLayout});
      allocated.push(this.vertexArray);
      this.vertices = new DynamicGpuBuffer(device, {id: 'shadow-colored-vertices', usage: Buffer.VERTEX | Buffer.COPY_DST,
        onReplace: buffer => this.vertexArray.setBuffer(0, buffer)});
    } catch(error) {
      for(const resource of allocated.reverse()) {
        resource.destroy();
      }
      throw error;
    }
  }

  /** Reset the view cursor once before recording a new frame; triangles are borrowed XYZ/RGBA. */
  prepare(triangles: number[] | Float32Array) {
    this.cursor = 0;
    this.vertexCount = triangles.length/7;
    this.vertices.write(triangles instanceof Float32Array ? triangles : new Float32Array(triangles));
  }

  /** Record one view. Each view keeps its own matrix until the frame is submitted. */
  draw(view: ShadowRenderView, drawMaterials?: (pass: RenderPass, matrix: Buffer) => void) {
    const index = this.cursor++;
    let matrix = this.matrices[index];
    if(!matrix) {
      matrix = this.device.createBuffer({id: 'shadow-view-matrix', byteLength: 64, usage: Buffer.UNIFORM | Buffer.COPY_DST});
      this.matrices.push(matrix);
    }
    matrix.write(new Float32Array(view.viewProjectionMatrix));
    if(this.vertexCount) {
      view.renderPass.setPipeline(this.pipeline);
      view.renderPass.setBindings({casterCamera: matrix});
      view.renderPass.setVertexArray(this.vertexArray);
      view.renderPass.draw({vertexCount: this.vertexCount});
    }
    drawMaterials?.(view.renderPass, matrix);
  }

  /** Release all recorded-view snapshots and geometry before the borrowed device. */
  destroy() {
    if(this.disposed) {
      return;
    }
    this.disposed = true;
    for(const buffer of this.matrices) {
      buffer.destroy();
    }
    this.vertices.destroy();
    this.vertexArray.destroy();
    this.pipeline.destroy();
    this.shader.vertex.destroy();
  }
}
