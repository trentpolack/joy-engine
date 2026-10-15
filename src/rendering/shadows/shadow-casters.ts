// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {
  Device,
  RenderPass,
  RenderPipeline,
  VertexArray
} from '@luma.gl/core';
import type { ShadowPassContext } from './scene-shadows.ts';
import type { ShadowOptions } from './types.ts';

import { Buffer } from '@luma.gl/core';
import { DynamicGpuBuffer } from '../gpu/dynamic-gpu-buffer.ts';
import { createPortableShaders } from '../gpu/gpu-device.ts';
import { DEFAULT_SHADOW_OPTIONS } from './types.ts';
import wgsl from '../../../shaders/rendering/shadows/shadow-caster.wgsl?raw';

/**
 * ShadowCasters Class Definition.
 *  Owns colored caster geometry and per-recorded-view matrix snapshots.
 *  PBR casters borrow those snapshots to share a light view without rewriting it.
 */
export class ShadowCasters {
  // Owned GPU resources; each recorded view retains its own uniform matrix.
  private readonly matrices: Buffer[] = [];
  private readonly shader: ReturnType<typeof createPortableShaders>;
  private readonly pipeline: RenderPipeline;
  private readonly vertexArray: VertexArray;
  private readonly vertices: DynamicGpuBuffer;

  // Borrowed device; its creator releases it after these resources are destroyed.
  private readonly device: Device;

  // Transient frame geometry, matrix cursor, and lifecycle state.
  private cursor = 0;
  private vertexCount = 0;
  private disposed = false;

  /**
   * Allocate a depth-only colored caster pipeline, releasing completed allocations on failure.
   * @param device Borrowed device, valid until destroy() completes.
   * @param options Borrowed raster bias settings; scalar values are copied into the pipeline.
   * @throws If shader creation or GPU allocation fails.
   */
  constructor(device: Device, options: ShadowOptions = {}) {
    this.device = device;
    const allocated: { destroy(): void }[] = [];

    try {
      this.shader = createPortableShaders(device, {
        wgsl,
        glsl: {
          vertex: '',
          fragment: ''
        }
      }, 'shadow-caster');
      allocated.push(this.shader.vertex);
      this.pipeline = device.createRenderPipeline({
        id: 'colored-shadow-caster',
        vs: this.shader.vertex,
        fs: this.shader.fragment,
        vertexEntryPoint: 'vertexMain',
        fragmentEntryPoint: 'fragmentMain',
        topology: 'triangle-list',
        colorAttachmentFormats: [],
        depthStencilAttachmentFormat: 'depth32float',
        bufferLayout: [{
          name: 'vertices',
          byteStride: 28,
          attributes: [{
            attribute: 'position',
            format: 'float32x3',
            byteOffset: 0
          }]
        }],
        parameters: {
          depthWriteEnabled: true,
          depthCompare: 'less-equal',
          cullMode: 'none',
          depthBias: options.depthBias ?? DEFAULT_SHADOW_OPTIONS.depthBias,
          depthBiasSlopeScale: options.depthBiasSlopeScale ?? DEFAULT_SHADOW_OPTIONS.depthBiasSlopeScale
        }
      });
      allocated.push(this.pipeline);
      this.vertexArray = device.createVertexArray({
        shaderLayout: this.pipeline.shaderLayout,
        bufferLayout: this.pipeline.bufferLayout
      });
      allocated.push(this.vertexArray);
      this.vertices = new DynamicGpuBuffer(device, {
        id: 'shadow-colored-vertices',
        usage: Buffer.VERTEX | Buffer.COPY_DST,
        onReplace: buffer => this.vertexArray.setBuffer(0, buffer)
      });
    } catch(error) {
      for(const resource of allocated.reverse()) {
        resource.destroy();
      }
      throw error;
    }
  }

  /**
   * Reset the view cursor and upload geometry once before recording a new frame.
   * @param triangles Borrowed complete world-space XYZ/RGBA triangles; copied to owned GPU storage without mutating the input.
   * @throws If geometry allocation or upload fails.
   */
  prepare(triangles: number[] | Float32Array) {
    this.cursor = 0;
    this.vertexCount = triangles.length/7;
    this.vertices.write(triangles instanceof Float32Array ? triangles : new Float32Array(triangles));
  }

  /**
   * Record colored casters, followed by optional material casters, for one light view.
   * Each recorded view keeps its own matrix until the frame is submitted.
   * @param view Borrowed active pass and frozen WebGPU camera; drawing updates pass state without ending or submitting it.
   * @param drawMaterials Optional synchronous recorder borrowing the pass and owned matrix buffer; it must not mutate or destroy the buffer, whose contents remain valid through this frame's submission.
   * @throws If matrix allocation, upload, or material recording fails.
   */
  draw(
    view: ShadowPassContext,
    drawMaterials?: (pass: RenderPass, matrix: Buffer) => void) {
    const index = this.cursor++;
    let matrix = this.matrices[index];
    if(!matrix) {
      matrix = this.device.createBuffer({
        id: 'shadow-view-matrix',
        byteLength: 64,
        usage: Buffer.UNIFORM | Buffer.COPY_DST
      });
      this.matrices.push(matrix);
    }
    matrix.write(new Float32Array(view.camera.viewProjectionMatrix));

    if(this.vertexCount) {
      view.renderPass.setPipeline(this.pipeline);
      view.renderPass.setBindings({ casterCamera: matrix });
      view.renderPass.setVertexArray(this.vertexArray);
      view.renderPass.draw({ vertexCount: this.vertexCount });
    }
    drawMaterials?.(view.renderPass, matrix);
  }

  /**
   * Release recorded-view snapshots and geometry before the borrowed device.
   * Repeated calls are harmless; matrix buffers previously given to callbacks become invalid.
   */
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
