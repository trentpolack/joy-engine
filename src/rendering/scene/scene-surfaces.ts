// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {
  GPU_ADDRESS_MODE,
  GPU_BACKEND,
  GPU_FILTER,
  GPU_TEXTURE_FORMAT,
  GPU_TOPOLOGY,
  GPU_VERTEX_FORMAT,
  SHADER_ENTRY_POINT
} from '../gpu/constants.ts';
import type { Device, Framebuffer, RenderPipeline, Shader, VertexArray } from '@luma.gl/core';
import { Buffer, Texture } from '@luma.gl/core';
import { DynamicGpuBuffer } from '../gpu/dynamic-gpu-buffer.ts';
import { FLOATS_PER_VERTEX } from '../geometry/geometry.ts';
import wgsl from '../../../shaders/rendering/scene/scene-surfaces.wgsl?raw';

const SURFACE_FORMAT = GPU_TEXTURE_FORMAT.RGBA16FLOAT;
const SURFACE_VERTEX_FLOATS = 4;
const MATRIX_FLOATS = 16;

/**
 * Owns the WebGPU normal/roughness attachment used by luma screen-space effects.
 * Borrows the device and opaque depth; never clears, writes or destroys scene depth.
 * RGB stores signed view-space face normals mapped to [0, 1], alpha perceptual
 * roughness. Existing vertex-color surfaces remain matte unless explicitly authored.
 */
export class SceneSurfaces {
  private declare device: Device;
  private declare shader: Shader | null;
  private declare pipeline: RenderPipeline | null;
  private declare vertexArray: VertexArray | null;
  private declare vertices: DynamicGpuBuffer | null;
  private declare uniforms: Buffer | null;
  private declare texture: Texture | null;
  private declare framebuffer: Framebuffer | null;
  private declare depth: Texture | null;
  declare destroyed: boolean;

  /** @param device Borrowed WebGPU device shared with the scene renderer. */
  constructor(device: Device) {
    this.device = device;
    this.shader = null;
    this.pipeline = null;
    this.vertexArray = null;
    this.vertices = null;
    this.uniforms = null;
    this.texture = null;
    this.framebuffer = null;
    this.depth = null;
    this.destroyed = false;

    if(device.type !== GPU_BACKEND.WEBGPU) {
      throw new Error('SceneSurfaces requires the WebGPU backend.');
    }

    try {
      this.shader = device.createShader({id: 'scene-surfaces', source: wgsl, language: 'wgsl'});
      this.pipeline = device.createRenderPipeline({
        id: 'scene-surfaces', vs: this.shader, fs: this.shader,
        vertexEntryPoint: SHADER_ENTRY_POINT.VERTEX, fragmentEntryPoint: SHADER_ENTRY_POINT.FRAGMENT,
        topology: GPU_TOPOLOGY.TRIANGLE_LIST,
        bufferLayout: [{
          name: 'vertices', byteStride: SURFACE_VERTEX_FLOATS*Float32Array.BYTES_PER_ELEMENT,
          attributes: [
            {attribute: 'position', format: GPU_VERTEX_FORMAT.FLOAT32X3, byteOffset: 0},
            {attribute: 'roughness', format: 'float32', byteOffset: 12}
          ]
        }],
        colorAttachmentFormats: [SURFACE_FORMAT],
        depthStencilAttachmentFormat: GPU_TEXTURE_FORMAT.DEPTH24PLUS,
        parameters: {depthWriteEnabled: false, depthCompare: 'equal', blend: false}
      });
      this.vertexArray = device.createVertexArray({
        id: 'scene-surface-vertices', shaderLayout: this.pipeline.shaderLayout,
        bufferLayout: this.pipeline.bufferLayout
      });
      this.vertices = new DynamicGpuBuffer(device, {
        id: 'scene-surface-vertices', usage: Buffer.VERTEX | Buffer.COPY_DST,
        onReplace: buffer => this.vertexArray?.setBuffer(0, buffer)
      });
      this.uniforms = device.createBuffer({
        id: 'scene-surface-uniforms', byteLength: MATRIX_FLOATS*2*Float32Array.BYTES_PER_ELEMENT,
        usage: Buffer.UNIFORM | Buffer.COPY_DST
      });
    } catch(error) {
      this.destroy();
      throw error;
    }
  }

  /**
   * Render after opaque depth is complete and before screen-space effects.
   * Inputs are borrowed; opaque vertices are the exact world XYZ/RGBA scene queue.
   * Matrices are column-major; viewProjection must include the scene's current jitter.
   * roughness has one [0, 1] perceptual value per vertex, defaulting to 1 (matte).
   * Returns a borrowed texture valid until resize or destroy. Caller submits GPU work.
   */
  render(vertices: number[] | Float32Array, viewProjection: Float32Array,
    viewMatrix: Float32Array, depthTexture: Texture, roughness?: ArrayLike<number>): Texture {
    if(this.destroyed || !this.pipeline || !this.vertexArray || !this.vertices || !this.uniforms) {
      throw new Error('SceneSurfaces is destroyed.');
    }
    const vertexCount = vertices.length/FLOATS_PER_VERTEX;
    if(vertices.length % (FLOATS_PER_VERTEX*3) !== 0 || (roughness && roughness.length !== vertexCount)) {
      throw new RangeError('Scene surfaces need complete XYZ/RGBA triangles and one roughness value per vertex.');
    }
    if(viewProjection.length !== MATRIX_FLOATS || viewMatrix.length !== MATRIX_FLOATS ||
      !viewProjection.every(Number.isFinite) || !viewMatrix.every(Number.isFinite)) {
      throw new RangeError('Scene surfaces need finite 4×4 view and view/projection matrices.');
    }

    const packed = new Float32Array(vertexCount*SURFACE_VERTEX_FLOATS);
    for(let vertex = 0; vertex < vertexCount; vertex++) {
      const source = vertex*FLOATS_PER_VERTEX;
      const target = vertex*SURFACE_VERTEX_FLOATS;
      const value = roughness?.[vertex] ?? 1;
      if(!Number.isFinite(value) || value < 0 || value > 1) {
        throw new RangeError('Surface roughness must be finite and between zero and one.');
      }
      packed.set([vertices[source], vertices[source + 1], vertices[source + 2], value], target);
    }
    if(!packed.every(Number.isFinite)) {
      throw new RangeError('Surface positions must fit finite float32 world coordinates.');
    }
    this.resize(depthTexture);
    this.vertices.write(packed);
    const uniformData = new Float32Array(MATRIX_FLOATS*2);
    uniformData.set(viewProjection);
    uniformData.set(viewMatrix, MATRIX_FLOATS);
    this.uniforms.write(uniformData);

    const pass = this.device.beginRenderPass({
      framebuffer: this.framebuffer,
      // A finite neutral normal avoids normalize(0) in neighborhood filters.
      // Background validity comes from the shared clear-depth value of 1.
      clearColor: [0.5, 0.5, 1, 1], clearDepth: false, depthReadOnly: true
    });
    try {
      pass.setPipeline(this.pipeline);
      pass.setBindings({uniforms: this.uniforms});
      pass.setVertexArray(this.vertexArray);
      if(vertexCount > 0) {
        pass.draw({vertexCount});
      }
    } finally {
      pass.end();
    }
    return this.texture!;
  }

  /**
   * Match a borrowed depth attachment's dimensions and identity without destroying it.
   * Transactional allocation leaves the last working target usable on failure.
   */
  resize(depthTexture: Texture) {
    if(this.destroyed) {
      throw new Error('SceneSurfaces is destroyed.');
    }
    const {width, height} = depthTexture;
    if(depthTexture.format !== GPU_TEXTURE_FORMAT.DEPTH24PLUS || !Number.isInteger(width) || !Number.isInteger(height) || width <= 0 || height <= 0) {
      throw new RangeError('Scene surfaces require a positive-size depth24plus scene attachment.');
    }
    if(this.depth === depthTexture && this.texture?.width === width && this.texture.height === height) {
      return;
    }
    let texture: Texture | null = null;
    let framebuffer: Framebuffer;
    try {
      texture = this.device.createTexture({
        id: 'scene-normal-roughness', width, height, format: SURFACE_FORMAT,
        usage: Texture.RENDER | Texture.SAMPLE,
        sampler: {minFilter: GPU_FILTER.NEAREST, magFilter: GPU_FILTER.NEAREST, addressModeU: GPU_ADDRESS_MODE.CLAMP_TO_EDGE, addressModeV: GPU_ADDRESS_MODE.CLAMP_TO_EDGE}
      });
      framebuffer = this.device.createFramebuffer({
        id: 'scene-surfaces', width, height, colorAttachments: [texture], depthStencilAttachment: depthTexture
      });
    } catch(error) {
      texture?.destroy();
      throw error;
    }
    this.framebuffer?.destroy();
    this.texture?.destroy();
    this.framebuffer = framebuffer;
    this.texture = texture;
    this.depth = depthTexture;
  }

  /** Release owned attachments and pipeline once; the scene device/depth remain alive. */
  destroy() {
    if(this.destroyed) {
      return;
    }
    this.destroyed = true;
    this.framebuffer?.destroy();
    this.texture?.destroy();
    this.vertexArray?.destroy();
    this.vertices?.destroy();
    this.uniforms?.destroy();
    this.pipeline?.destroy();
    this.shader?.destroy();
    this.depth = null;
    this.framebuffer = null;
    this.texture = null;
  }
}
