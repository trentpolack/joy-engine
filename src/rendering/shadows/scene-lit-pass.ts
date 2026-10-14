// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {Buffer} from '@luma.gl/core';
import type {Device, RenderPass, RenderPipeline, TextureFormatColor, VertexArray} from '@luma.gl/core';
import {shadow} from '@luma.gl/experimental';
import {createPortableShaders} from '../gpu/gpu-device.ts';
import {POINT_LIGHT_UNIFORM_BYTES} from '../lighting/point-lights.ts';
import {SceneShadows} from './scene-shadows.ts';
import type {PreparedShadowBatch} from './scene-shadows.ts';
import {ShadowCasters} from './shadow-casters.ts';
import type {SceneLightFrame, ShadowOptions} from './types.ts';
import wgsl from '../../../shaders/rendering/shadows/lit-shadow.wgsl?raw';

/** Owns shadowed vertex-color lighting; borrows the main scene's geometry and pass. */
export class SceneLitPass {
  readonly shadows: SceneShadows;
  private readonly casters: ShadowCasters;
  private readonly shaders: ReturnType<typeof createPortableShaders>;
  private readonly opaque: RenderPipeline;
  private readonly additive: RenderPipeline;
  private readonly receiver: Buffer;
  private readonly uniforms: Buffer[] = [];
  private batches: PreparedShadowBatch[] = [];
  private disposed = false;
  private readonly device: Device;

  constructor(device: Device, options: ShadowOptions = {}, colorFormat?: TextureFormatColor) {
    this.device = device;
    const allocated: {destroy(): void}[] = [];
    try {
      this.shadows = new SceneShadows(device, options);
      allocated.push(this.shadows);
      this.casters = new ShadowCasters(device, options);
      allocated.push(this.casters);
      this.shaders = createPortableShaders(device, {wgsl, glsl: {vertex: '', fragment: ''}, modules: [shadow]}, 'lit-shadow');
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

  /** Record maps and snapshot each direct-light group; ambient/directional irradiance stays in group zero. */
  prepare(frame: SceneLightFrame, source: Float32Array, drawMaterials?: (pass: RenderPass, matrix: Buffer) => void) {
    this.casters.prepare(frame.casters);
    this.batches = this.shadows.prepare(frame.camera, frame.lights, frame.directional, view => this.casters.draw(view, drawMaterials));
    this.receiver.write(new Float32Array(Array.from(frame.camera.viewMatrix)));
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
  draw(pass: RenderPass, vertexArray: VertexArray, vertexCount: number) {
    for(const [index, batch] of this.batches.entries()) {
      pass.setPipeline(index === 0 ? this.opaque : this.additive);
      pass.setVertexArray(vertexArray);
      pass.setBindings({uniforms: this.uniforms[index], receiver: this.receiver, ...batch.bindings});
      pass.draw({vertexCount});
    }
  }

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
}
