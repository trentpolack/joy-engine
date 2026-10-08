// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {
  GPU_ADDRESS_MODE,
  GPU_BACKEND,
  GPU_COMPARE,
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
import wgsl from '../../../shaders/rendering/scene/scene-motion.wgsl?raw';

export interface SceneMotionFrame {
  vertices: readonly number[] | Float32Array;
  previousPositions: Float32Array;
  currentViewProjection: ArrayLike<number>;
  previousViewProjection: ArrayLike<number>;
  rasterViewProjection?: ArrayLike<number>;
  /** Current minus previous projection jitter in top-left-origin UV units. */
  velocityJitterDelta?: readonly [number, number];
  width: number;
  height: number;
}

interface MotionTargets {
  unjittered: Texture;
  jittered: Texture;
  depth: Texture;
  framebuffer: Framebuffer;
}

const MOTION_FORMAT = GPU_TEXTURE_FORMAT.RGBA16FLOAT;
const UNIFORM_FLOATS = 52;

/**
 * Owns WebGPU signed current-minus-previous UV motion attachments for opaque triangles.
 * The borrowed device submits work. Render re-rasterizes the same opaque queue with its
 * own depth buffer, preserving nearest-surface occlusion without changing scene depth.
 * Both upstream jitter conventions share one geometry draw and two color attachments.
 */
export class SceneMotionPass {
  private declare device: Device;
  private declare shader: Shader | null;
  private declare pipeline: RenderPipeline | null;
  private declare vertexArray: VertexArray | null;
  private declare vertices: DynamicGpuBuffer | null;
  private declare previousPositions: DynamicGpuBuffer | null;
  private declare uniforms: Buffer | null;
  private declare targets: MotionTargets | null;
  declare destroyed: boolean;

  constructor(device: Device) {
    this.device = device;
    this.shader = null;
    this.pipeline = null;
    this.vertexArray = null;
    this.vertices = null;
    this.previousPositions = null;
    this.uniforms = null;
    this.targets = null;
    this.destroyed = false;
    if(device.type !== GPU_BACKEND.WEBGPU) {
      throw new Error('Scene motion attachments require the WebGPU backend.');
    }
    try {
      this.shader = device.createShader({id: 'scene-motion', source: wgsl, language: 'wgsl'});
      this.pipeline = device.createRenderPipeline({
        id: 'scene-motion-pipeline',
        vs: this.shader,
        fs: this.shader,
        vertexEntryPoint: SHADER_ENTRY_POINT.VERTEX,
        fragmentEntryPoint: SHADER_ENTRY_POINT.FRAGMENT,
        topology: GPU_TOPOLOGY.TRIANGLE_LIST,
        bufferLayout: [
          {name: 'vertices', byteStride: FLOATS_PER_VERTEX*4,
            attributes: [{attribute: 'position', format: GPU_VERTEX_FORMAT.FLOAT32X3, byteOffset: 0}]},
          {name: 'previousPositions', byteStride: 12,
            attributes: [{attribute: 'previousPosition', format: GPU_VERTEX_FORMAT.FLOAT32X3, byteOffset: 0}]}
        ],
        colorAttachmentFormats: [MOTION_FORMAT, MOTION_FORMAT],
        depthStencilAttachmentFormat: GPU_TEXTURE_FORMAT.DEPTH24PLUS,
        parameters: {depthWriteEnabled: true, depthCompare: GPU_COMPARE.LESS_EQUAL, blend: false}
      });
      this.vertexArray = device.createVertexArray({
        id: 'scene-motion-vertex-array',
        shaderLayout: this.pipeline.shaderLayout,
        bufferLayout: this.pipeline.bufferLayout
      });
      this.vertices = new DynamicGpuBuffer(device, {
        id: 'scene-motion-vertices', usage: Buffer.VERTEX | Buffer.COPY_DST,
        onReplace: buffer => this.vertexArray?.setBuffer(0, buffer)
      });
      this.previousPositions = new DynamicGpuBuffer(device, {
        id: 'scene-motion-previous-positions', usage: Buffer.VERTEX | Buffer.COPY_DST,
        onReplace: buffer => this.vertexArray?.setBuffer(1, buffer)
      });
      this.uniforms = device.createBuffer({
        id: 'scene-motion-uniforms', byteLength: UNIFORM_FLOATS*4, usage: Buffer.UNIFORM | Buffer.COPY_DST
      });
    } catch(error) {
      this.destroy();
      throw error;
    }
  }

  /** Borrowed output with projection jitter included; valid until resize or destruction. */
  get jitteredVelocityTexture(): Texture | null {
    return this.targets?.jittered ?? null;
  }

  /**
   * Upload borrowed current XYZ/RGBA vertices and matching previous world-space XYZ values.
   * Camera matrices are column-major and unjittered; rasterViewProjection may contain jitter.
   * Returns the owned unjittered motion texture for upstream TAA and motion blur. No submit.
   */
  render(frame: SceneMotionFrame): Texture {
    if(this.destroyed || !this.pipeline || !this.vertexArray || !this.uniforms) {
      throw new Error('SceneMotionPass is destroyed.');
    }
    validateFrame(frame);
    this.resize(frame.width, frame.height);
    const targets = this.targets!;
    this.vertices!.write(new Float32Array(frame.vertices));
    this.previousPositions!.write(frame.previousPositions);
    const uniforms = new Float32Array(UNIFORM_FLOATS);
    uniforms.set(Array.from(frame.currentViewProjection), 0);
    uniforms.set(Array.from(frame.previousViewProjection), 16);
    uniforms.set(Array.from(frame.rasterViewProjection ?? frame.currentViewProjection), 32);
    uniforms.set(frame.velocityJitterDelta ?? [0, 0], 48);
    this.uniforms.write(uniforms);

    const pass = this.device.beginRenderPass({
      framebuffer: targets.framebuffer, clearColor: [0, 0, 0, 0], clearDepth: 1
    });
    try {
      pass.setPipeline(this.pipeline);
      pass.setBindings({uniforms: this.uniforms});
      pass.setVertexArray(this.vertexArray);
      pass.draw({vertexCount: frame.vertices.length/FLOATS_PER_VERTEX});
    } finally {
      pass.end();
    }
    return targets.unjittered;
  }

  /** Release owned resources once; the borrowed device remains alive. */
  destroy() {
    if(this.destroyed) {
      return;
    }
    this.destroyed = true;
    destroyTargets(this.targets);
    this.targets = null;
    this.vertexArray?.destroy();
    this.vertices?.destroy();
    this.previousPositions?.destroy();
    this.uniforms?.destroy();
    this.pipeline?.destroy();
    this.shader?.destroy();
  }

  /** Allocate transactionally so a failed resize retains the previous complete target set. */
  private resize(width: number, height: number) {
    if(this.targets?.unjittered.width === width && this.targets.unjittered.height === height) {
      return;
    }
    const owned: Texture[] = [];
    try {
      const createColor = (id: string) => {
        const texture = this.device.createTexture({
          id, width, height, format: MOTION_FORMAT, usage: Texture.RENDER | Texture.SAMPLE,
          sampler: {minFilter: GPU_FILTER.LINEAR, magFilter: GPU_FILTER.LINEAR, addressModeU: GPU_ADDRESS_MODE.CLAMP_TO_EDGE, addressModeV: GPU_ADDRESS_MODE.CLAMP_TO_EDGE}
        });
        owned.push(texture);
        return texture;
      };
      const unjittered = createColor('scene-motion-unjittered');
      const jittered = createColor('scene-motion-jittered');
      const depth = this.device.createTexture({
        id: 'scene-motion-depth', width, height, format: GPU_TEXTURE_FORMAT.DEPTH24PLUS, usage: Texture.RENDER
      });
      owned.push(depth);
      const framebuffer = this.device.createFramebuffer({
        id: 'scene-motion-framebuffer', width, height,
        colorAttachments: [unjittered, jittered], depthStencilAttachment: depth
      });
      destroyTargets(this.targets);
      this.targets = {unjittered, jittered, depth, framebuffer};
    } catch(error) {
      for(const texture of owned) {
        texture.destroy();
      }
      throw error;
    }
  }
}

function destroyTargets(targets: MotionTargets | null) {
  targets?.framebuffer.destroy();
  targets?.unjittered.destroy();
  targets?.jittered.destroy();
  targets?.depth.destroy();
}

function validateFrame(frame: SceneMotionFrame) {
  if(!Number.isInteger(frame.width) || frame.width <= 0 || !Number.isInteger(frame.height) || frame.height <= 0) {
    throw new RangeError('Scene motion dimensions must be positive integers.');
  }
  if(frame.vertices.length % (FLOATS_PER_VERTEX*3) ||
    frame.previousPositions.length !== frame.vertices.length/FLOATS_PER_VERTEX*3) {
    throw new RangeError('Scene motion requires matching current and previous complete triangles.');
  }
  for(const matrix of [frame.currentViewProjection, frame.previousViewProjection, frame.rasterViewProjection ?? frame.currentViewProjection]) {
    if(matrix.length !== 16 || !Array.from(matrix).every(Number.isFinite)) {
      throw new RangeError('Scene motion camera matrices must contain 16 finite values.');
    }
  }
  if(frame.velocityJitterDelta && (frame.velocityJitterDelta.length !== 2 || !frame.velocityJitterDelta.every(Number.isFinite))) {
    throw new RangeError('Scene motion jitter delta must contain two finite UV values.');
  }
}
