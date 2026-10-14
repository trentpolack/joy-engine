// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {Buffer} from '@luma.gl/core';
import type {Binding, Device, RenderPass, RenderPipeline, TextureFormatColor, VertexArray} from '@luma.gl/core';
import {sceneShadowModule as shadow} from './scene-shadow-module.ts';
import {createPortableShaders} from '../gpu/gpu-device.ts';
import {POINT_LIGHT_UNIFORM_BYTES} from '../lighting/point-lights.ts';
import type {PreparedShadowBatch} from './scene-shadows.ts';
import type {ShadowCameraFrame} from './types.ts';
import wgsl from '../../../shaders/rendering/shadows/lit-shadow.wgsl?raw';
import lighting from '../../../shaders/rendering/scene/scene-lighting.wgsl?raw';

/** Owns shadowed vertex-color lighting; borrows the main scene's geometry and pass. */
export class SceneLitPass {
  private readonly shaders: ReturnType<typeof createPortableShaders>;
  private readonly opaque: RenderPipeline;
  private readonly additive: RenderPipeline;
  private readonly receiver: Buffer;
  private readonly uniforms: Buffer[] = [];
  private batches: readonly PreparedShadowBatch[] = [];
  private disposed = false;
  private readonly device: Device;

  constructor(device: Device, colorFormat?: TextureFormatColor) {
    this.device = device;
    const allocated: {destroy(): void}[] = [];
    try {
      this.shaders = createPortableShaders(device, {wgsl: lighting + wgsl, glsl: {vertex: '', fragment: ''}, modules: [shadow], defines: {JOY_SHADOWS: true}}, 'lit-shadow');
      allocated.push(this.shaders.vertex);
      const pipelineOptions = {
        vs: this.shaders.vertex, fs: this.shaders.fragment, vertexEntryPoint: 'vertexMain', fragmentEntryPoint: 'fragmentMain',
        topology: 'triangle-list' as const, depthStencilAttachmentFormat: 'depth24plus' as const,
        ...(colorFormat ? {colorAttachmentFormats: [colorFormat]} : {}),
        bufferLayout: [{name: 'vertices', byteStride: 28, attributes: [
          {attribute: 'position', format: 'float32x3' as const, byteOffset: 0},
          {attribute: 'color', format: 'float32x4' as const, byteOffset: 12}
        ]}]
      };
      this.opaque = device.createRenderPipeline({...pipelineOptions, id: 'lit-shadow-base',
        parameters: {depthWriteEnabled: true, depthCompare: 'less-equal', cullMode: 'none'}});
      allocated.push(this.opaque);
      this.additive = device.createRenderPipeline({...pipelineOptions, id: 'lit-shadow-additive', parameters: {
        depthWriteEnabled: false, depthCompare: 'equal', cullMode: 'none', blend: true,
        blendColorOperation: 'add', blendAlphaOperation: 'add', blendColorSrcFactor: 'one', blendColorDstFactor: 'one',
        blendAlphaSrcFactor: 'zero', blendAlphaDstFactor: 'one'
      }});
      allocated.push(this.additive);
      this.receiver = device.createBuffer({id: 'shadow-receiver-camera', byteLength: 64, usage: Buffer.UNIFORM | Buffer.COPY_DST});
    } catch(error) {
      for(const resource of allocated.reverse()) {
        resource.destroy();
      }
      throw error;
    }
  }

  /** Borrow frame maps and snapshot each direct-light group; indirect illumination stays in group zero. */
  prepare(camera: ShadowCameraFrame, source: Float32Array, batches: readonly PreparedShadowBatch[]) {
    this.batches = batches;
    this.receiver.write(new Float32Array(Array.from(camera.viewMatrix)));
    for(const [index, batch] of this.batches.entries()) {
      const values = source.slice(0, POINT_LIGHT_UNIFORM_BYTES/4);
      values[20] = batch.lights.length;
      values.fill(0, 24, 280);
      batch.lights.forEach((light, slot) => {
        values.set([light.x, light.y, light.z, light.range, light.r, light.g, light.b, batch.shadowIndices[slot] + 1], 24 + slot*8);
      });
      if(index > 0) {
        values[21] = 0;
        values.fill(0, 280);
      }
      let buffer = this.uniforms[index];
      if(!buffer) {
        buffer = this.device.createBuffer({id: 'shadow-lit-group', byteLength: POINT_LIGHT_UNIFORM_BYTES, usage: Buffer.UNIFORM | Buffer.COPY_DST});
        this.uniforms.push(buffer);
      }
      buffer.write(values);
    }
  }

  /** Draw opaque triangles once for base illumination, then accumulate remaining direct-light groups. */
  draw(pass: RenderPass, vertexArray: VertexArray, vertexCount: number, firstVertex: number = 0) {
    for(const [index, batch] of this.batches.entries()) {
      pass.setPipeline(index === 0 ? this.opaque : this.additive);
      pass.setVertexArray(vertexArray);
      pass.setBindings({uniforms: this.uniforms[index], receiver: this.receiver, ...batch.bindings});
      pass.draw({vertexCount, firstVertex});
    }
  }

  /** Native adapters borrow the same per-batch lighting snapshots and maps. */
  drawNative(pass: RenderPass, draw: (pass: RenderPass, bindings: Record<string, Binding>, additive: boolean) => void) {
    for(const [index, batch] of this.batches.entries()) {
      draw(pass, {uniforms: this.uniforms[index], receiver: this.receiver, ...batch.bindings}, index > 0);
    }
  }

  destroy() {
    if(this.disposed) {
      return;
    }
    this.disposed = true;
    for(const buffer of this.uniforms) {
      buffer.destroy();
    }
    this.receiver.destroy();
    this.opaque.destroy();
    this.additive.destroy();
    this.shaders.vertex.destroy();
  }
}
