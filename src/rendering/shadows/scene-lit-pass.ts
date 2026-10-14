// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {
  Device,
  RenderPass,
  RenderPipeline,
  TextureFormatColor,
  VertexArray
} from '@luma.gl/core';
import type { PreparedShadowBatch } from './scene-shadows.ts';
import type {
  SceneLightFrame,
  ShadowOptions
} from './types.ts';

import { Buffer } from '@luma.gl/core';
import { shadow } from '@luma.gl/experimental';
import { createPortableShaders } from '../gpu/gpu-device.ts';
import { POINT_LIGHT_UNIFORM_BYTES } from '../lighting/point-lights.ts';
import { SceneShadows } from './scene-shadows.ts';
import { ShadowCasters } from './shadow-casters.ts';
import wgsl from '../../../shaders/rendering/shadows/lit-shadow.wgsl?raw';

/**
 * SceneLitPass Class Definition.
 *  Owns shadowed vertex-color lighting; borrows the main scene's geometry and pass.
 */
export class SceneLitPass {
  // Owned shadow resources, receiver pipelines, and independent batch uniforms.
  readonly shadows: SceneShadows;
  private readonly casters: ShadowCasters;
  private readonly shaders: ReturnType<typeof createPortableShaders>;
  private readonly opaque: RenderPipeline;
  private readonly additive: RenderPipeline;
  private readonly receiver: Buffer;
  private readonly uniforms: Buffer[] = [];

  // Borrowed device; its creator releases it after this pass is destroyed.
  private readonly device: Device;

  // Prepared frame bindings and lifecycle state.
  private batches: PreparedShadowBatch[] = [];
  private disposed = false;

  /**
   * Allocate shadow and receiver resources, releasing completed allocations on failure.
   * @param device Borrowed WebGPU device, valid through destroy().
   * @param options Borrowed shadow settings; retained by the shadow owner as a copy.
   * @param colorFormat Optional scene color-attachment format; omitted uses the device default.
   * @throws If shadow settings or backend are unsupported, or GPU allocation fails.
   */
  constructor(
    device: Device,
    options: ShadowOptions = {},
    colorFormat?: TextureFormatColor) {
    this.device = device;
    const allocated: { destroy(): void }[] = [];

    try {
      this.shadows = new SceneShadows(device, options);
      allocated.push(this.shadows);
      this.casters = new ShadowCasters(device, options);
      allocated.push(this.casters);
      this.shaders = createPortableShaders(device, {
        wgsl,
        glsl: {
          vertex: '',
          fragment: ''
        },
        modules: [shadow]
      }, 'lit-shadow');
      allocated.push(this.shaders.vertex);

      const pipelineOptions = {
        vs: this.shaders.vertex,
        fs: this.shaders.fragment,
        vertexEntryPoint: 'vertexMain',
        fragmentEntryPoint: 'fragmentMain',
        topology: 'triangle-list' as const,
        depthStencilAttachmentFormat: 'depth24plus' as const,
        ...(colorFormat ? { colorAttachmentFormats: [colorFormat] } : {}),
        bufferLayout: [{
          name: 'vertices',
          byteStride: 28,
          attributes: [
            {
              attribute: 'position',
              format: 'float32x3' as const,
              byteOffset: 0
            },
            {
              attribute: 'color',
              format: 'float32x4' as const,
              byteOffset: 12
            }
          ]
        }]
      };
      this.opaque = device.createRenderPipeline({
        ...pipelineOptions,
        id: 'lit-shadow-base',
        parameters: {
          depthWriteEnabled: true,
          depthCompare: 'less-equal',
          cullMode: 'none'
        }
      });
      allocated.push(this.opaque);
      this.additive = device.createRenderPipeline({
        ...pipelineOptions,
        id: 'lit-shadow-additive',
        parameters: {
          depthWriteEnabled: false,
          depthCompare: 'equal',
          cullMode: 'none',
          blend: true,
          blendColorOperation: 'add',
          blendAlphaOperation: 'add',
          blendColorSrcFactor: 'one',
          blendColorDstFactor: 'one',
          blendAlphaSrcFactor: 'zero',
          blendAlphaDstFactor: 'one'
        }
      });
      allocated.push(this.additive);
      this.receiver = device.createBuffer({
        id: 'shadow-receiver-camera',
        byteLength: 64,
        usage: Buffer.UNIFORM | Buffer.COPY_DST
      });
    } catch(error) {
      for(const resource of allocated.reverse()) {
        resource.destroy();
      }
      throw error;
    }
  }

  /**
   * Record maps and snapshot each direct-light group without submitting the frame.
   * Ambient and directional irradiance remain in group zero; borrowed CPU inputs are unchanged.
   * @param frame Borrowed immutable camera/lighting view and complete world-space XYZ/RGBA caster triangles.
   * @param source Borrowed packed scene uniforms, copied separately into each batch's owned buffer.
   * @param drawMaterials Optional synchronous caster recorder borrowing a live pass and immutable-per-frame matrix buffer; it must not end the pass or destroy the buffer.
   * @throws If shadow preparation, resource allocation, or the material recorder fails.
   */
  prepare(
    frame: SceneLightFrame,
    source: Float32Array,
    drawMaterials?: (pass: RenderPass, matrix: Buffer) => void) {
    this.casters.prepare(frame.casters);
    this.batches = this.shadows.prepare(
      frame.view.camera,
      frame.view.lighting.lights,
      frame.view.lighting.directional,
      view => this.casters.draw(view, drawMaterials)
    );
    this.receiver.write(new Float32Array(frame.view.camera.viewMatrix));

    for(const [index, batch] of this.batches.entries()) {
      this.prepareBatchUniform(index, batch, source);
    }
  }

  /**
   * Draw base illumination once, then accumulate remaining direct-light groups in order.
   * @param pass Borrowed active scene pass; its draw state is updated, but it is not ended or submitted.
   * @param vertexArray Borrowed world-space XYZ/RGBA geometry, valid through frame submission.
   * @param vertexCount Number of vertices to draw for every prepared lighting batch.
   */
  draw(pass: RenderPass, vertexArray: VertexArray, vertexCount: number) {
    for(const [index, batch] of this.batches.entries()) {
      pass.setPipeline(index === 0 ? this.opaque : this.additive);
      pass.setVertexArray(vertexArray);
      pass.setBindings({
        uniforms: this.uniforms[index],
        receiver: this.receiver,
        ...batch.bindings
      });
      pass.draw({ vertexCount });
    }
  }

  /**
   * Release owned maps, geometry, uniforms, pipelines, and shaders before the borrowed device.
   * Repeated calls are harmless; previously prepared bindings are no longer valid.
   */
  destroy() {
    if(this.disposed) {
      return;
    }
    this.disposed = true;

    this.shadows.destroy();
    this.casters.destroy();
    for(const buffer of this.uniforms) {
      buffer.destroy();
    }
    this.receiver.destroy();
    this.opaque.destroy();
    this.additive.destroy();
    this.shaders.vertex.destroy();
  }

  /**
   * Copy one group's direct lights and upload its independent uniform snapshot.
   * @param index Batch index; additional batches exclude ambient and directional contributions.
   * @param batch Borrowed lights and shadow slots, unchanged by this method.
   * @param source Borrowed packed uniforms, copied before clearing or replacing fields.
   * @throws If uniform allocation or upload fails.
   */
  private prepareBatchUniform(
    index: number,
    batch: PreparedShadowBatch,
    source: Float32Array): void {
    const values = source.slice(0, POINT_LIGHT_UNIFORM_BYTES/4);
    values[20] = batch.lights.length;
    values.fill(0, 24, 280);

    for(const [slot, light] of batch.lights.entries()) {
      values.set([
        light.x,
        light.y,
        light.z,
        light.range,
        light.r,
        light.g,
        light.b,
        batch.shadowIndices[slot] + 1
      ], 24 + (slot*8));
    }

    if(index > 0) {
      values[21] = 0;
      values.fill(0, 280);
    }

    let buffer = this.uniforms[index];
    if(!buffer) {
      buffer = this.device.createBuffer({
        id: 'shadow-lit-group',
        byteLength: POINT_LIGHT_UNIFORM_BYTES,
        usage: Buffer.UNIFORM | Buffer.COPY_DST
      });
      this.uniforms.push(buffer);
    }
    buffer.write(values);
  }
}
