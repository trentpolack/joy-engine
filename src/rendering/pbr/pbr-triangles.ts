// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { GPU_BACKEND, GPU_COMPARE, GPU_TEXTURE_FORMAT, GPU_TOPOLOGY, GPU_VERTEX_FORMAT, SHADER_ENTRY_POINT } from '../gpu/constants.ts';
import { MATERIAL_ALPHA_MODE } from './constants.ts';
import type { TextureFormatColor, RenderPipelineProps, RenderPass, Device, Shader, RenderPipeline, VertexArray } from '@luma.gl/core';
import type { PbrMesh, PbrFrame, PbrMaterial } from '../../core/types.ts';
import { Buffer } from '@luma.gl/core';
import { createPortableShaders } from '../gpu/gpu-device.ts';
import { preparePbrGeometry } from '../geometry/pbr-geometry.ts';
import { PbrMaterialSet } from './pbr-materials.ts';
import wgsl from '../../../shaders/rendering/pbr/pbr.wgsl?raw';
import vertex from '../../../shaders/rendering/pbr/pbr.vert.glsl?raw';
import fragment from '../../../shaders/rendering/pbr/pbr.frag.glsl?raw';

export interface PbrTransparentRange {
  material: number;
  firstIndex: number;
  indexCount: number;
  depth: number;
}

/** Default inspection lighting in world space; consumers may override every field per frame. */
export const DEFAULT_PBR_LIGHTING = Object.freeze({
  lightDirection: Object.freeze([0.35, 0.85, 0.4]),
  lightColor: Object.freeze([3, 3, 3]),
  ambientColor: Object.freeze([0.22, 0.25, 0.3])
});

/** Optional material pass. Owns geometry, shaders and textures; borrows device and scene pass. */
export class PbrTriangles {
  declare device: Device;
  declare displayResolve: boolean;
  declare disposed: boolean;
  declare revision: number;
  declare materials: PbrMaterialSet;
  declare definitions: PbrMaterial[];
  declare triangles: { firstVertex: number; material: number; center: number[]; }[];
  declare runs: { material: number; firstIndex: number; indexCount: number; transparent: boolean; }[];
  declare transparentRanges: PbrTransparentRange[];
  declare vertices: Buffer | null;
  declare indices: Buffer | null;
  declare shaders: ReturnType<typeof createPortableShaders>;
  declare opaquePipeline: RenderPipeline;
  declare transparentPipeline: RenderPipeline;
  declare vertexArray: VertexArray;
  declare uniforms: Buffer;

  /** @param device @param [colorFormat] */
  constructor(device: Device, colorFormat?: TextureFormatColor) {
    this.device = device;
    this.displayResolve = !colorFormat;
    this.disposed = false;
    this.revision = 0;
    this.materials = new PbrMaterialSet(device);

    this.definitions = [];

    this.triangles = [];

    this.runs = [];
    /** Borrowed index ranges for mixed scene sorting. */
    this.transparentRanges = [];

    this.vertices = null;

    this.indices = null;

    const allocated: {
    destroy(): void;
}[] = [this.materials];
    try {
      this.shaders = createPortableShaders(device, {wgsl, glsl: {vertex, fragment}}, 'pbr-triangles');
      allocated.push(...new Set(Object.values(this.shaders)));

      const options: RenderPipelineProps = {
        vs: this.shaders.vertex, fs: this.shaders.fragment,
        vertexEntryPoint: SHADER_ENTRY_POINT.VERTEX, fragmentEntryPoint: SHADER_ENTRY_POINT.FRAGMENT, topology: GPU_TOPOLOGY.TRIANGLE_LIST,
        bufferLayout: [{name: 'vertices', byteStride: 48, attributes: [
          {attribute: 'position', format: GPU_VERTEX_FORMAT.FLOAT32X3, byteOffset: 0},
          {attribute: 'normal', format: GPU_VERTEX_FORMAT.FLOAT32X3, byteOffset: 12},
          {attribute: 'color', format: GPU_VERTEX_FORMAT.FLOAT32X4, byteOffset: 24},
          {attribute: 'uv', format: GPU_VERTEX_FORMAT.FLOAT32X2, byteOffset: 40}
        ]}],
        depthStencilAttachmentFormat: GPU_TEXTURE_FORMAT.DEPTH24PLUS,
        ...(colorFormat ? {colorAttachmentFormats: [colorFormat]} : {})
      };
      this.opaquePipeline = device.createRenderPipeline({
        ...options, id: 'pbr-opaque',
        parameters: {depthWriteEnabled: true, depthCompare: GPU_COMPARE.LESS_EQUAL, blend: false, cullMode: 'none'}
      });
      allocated.push(this.opaquePipeline);
      this.transparentPipeline = device.createRenderPipeline({
        ...options, id: 'pbr-transparent',
        parameters: {
          depthWriteEnabled: false, depthCompare: GPU_COMPARE.LESS_EQUAL, cullMode: 'none', blend: true,
          blendColorOperation: 'add', blendAlphaOperation: 'add',
          blendColorSrcFactor: 'src-alpha', blendColorDstFactor: 'one-minus-src-alpha',
          blendAlphaSrcFactor: 'one', blendAlphaDstFactor: 'one-minus-src-alpha'
        }
      });
      allocated.push(this.transparentPipeline);
      this.vertexArray = device.createVertexArray({shaderLayout: this.opaquePipeline.shaderLayout, bufferLayout: this.opaquePipeline.bufferLayout});
      allocated.push(this.vertexArray);
      this.uniforms = device.createBuffer({id: 'pbr-frame', byteLength: 128, usage: Buffer.UNIFORM | Buffer.COPY_DST});
    } catch(error) {
      for(const resource of allocated.reverse()) {
        resource.destroy();
      }
      throw error;
    }
  }

  /** Copy and upload geometry once; replace textures/materials atomically after decode.
   * Superseded requests return false. Failure preserves the previous mesh and material set.
   * @param mesh */
  async setMesh(mesh: PbrMesh): Promise<boolean> {
    if(this.disposed) {
      return false;
    }
    const revision = ++this.revision;
    const prepared = preparePbrGeometry(mesh);
    const definitions = structuredClone(mesh.materials);

    const pending: {
    destroy(): void;
}[] = [];

    const previous: {
    destroy(): void;
}[] = [];
    let installed = false;
    try {
      const vertices = this.device.createBuffer({id: 'pbr-vertices', byteLength: Math.max(4, prepared.vertices.byteLength), usage: Buffer.VERTEX | Buffer.COPY_DST});
      pending.push(vertices);
      const indices = this.device.createBuffer({id: 'pbr-indices', byteLength: Math.max(4, mesh.triangles.length*4), usage: Buffer.INDEX | Buffer.COPY_DST, indexType: 'uint32'});
      pending.push(indices);
      const vertexArray = this.device.createVertexArray({shaderLayout: this.opaquePipeline.shaderLayout, bufferLayout: this.opaquePipeline.bufferLayout});
      pending.push(vertexArray);
      if(prepared.vertices.length) {
        vertices.write(prepared.vertices);
      }
      vertexArray.setIndexBuffer(indices);
      vertexArray.setBuffer(0, vertices);
      if(this.device.type === GPU_BACKEND.WEBGL) {
        for(let attribute = 1; attribute < 4; attribute++) {
          vertexArray.setBuffer(attribute, vertices);
        }
      }
      // Keep geometry and material bindings paired even when callers supersede a
      // texture-free update before its promise continuation gets a turn to run.
      return await this.materials.update(definitions, () => {
        if(this.disposed || revision !== this.revision) {
          return false;
        }
        previous.push(this.vertexArray);
        if(this.vertices) {
          previous.push(this.vertices);
        }
        if(this.indices) {
          previous.push(this.indices);
        }
        this.vertices = vertices;
        this.indices = indices;
        this.vertexArray = vertexArray;
        this.triangles = prepared.triangles;
        this.definitions = [...definitions];
        this.runs = [];
        this.transparentRanges = [];
        installed = true;
        return true;
      });
    } finally {
      const release = installed ? previous : pending.reverse();
      for(const resource of release) {
        resource.destroy();
      }
    }
  }

  /** Update camera/lighting and triangle order without regenerating normals or uploading vertices.
   * @param frame
   * @param [includeTransparentRanges] Build per-triangle ranges for mixed material sorting.
   */
  prepare(frame: PbrFrame, includeTransparentRanges: boolean = false) {
    const values = new Float32Array(32);
    values.set(frame.viewProjection);
    values.set(frame.eye, 16);
    values.set(frame.lightDirection ?? DEFAULT_PBR_LIGHTING.lightDirection, 20);
    values.set(frame.lightColor ?? DEFAULT_PBR_LIGHTING.lightColor, 24);
    values.set(frame.ambientColor ?? DEFAULT_PBR_LIGHTING.ambientColor, 28);
    values[31] = this.displayResolve ? 1 : 0;
    this.uniforms.write(values);
    const opaque = this.triangles.filter(triangle => this.definitions[triangle.material].alphaMode !== MATERIAL_ALPHA_MODE.BLEND);
    const transparent = this.triangles.filter(triangle => this.definitions[triangle.material].alphaMode === MATERIAL_ALPHA_MODE.BLEND);
    opaque.sort((a, b) => a.material - b.material);
    // OrbitCamera.backward points toward the viewer: smaller projections are farther away.
    const depth = (center: number[]) => center.reduce((total, value, axis) => total + value*frame.backward[axis], 0);
    transparent.sort((a, b) => depth(a.center) - depth(b.center));
    const indices = new Uint32Array(this.triangles.length*3);
    this.runs = [];
    this.transparentRanges = [];
    let offset = 0;
    for(const triangle of [...opaque, ...transparent]) {
      indices.set([triangle.firstVertex, triangle.firstVertex + 1, triangle.firstVertex + 2], offset);
      if(includeTransparentRanges && this.definitions[triangle.material].alphaMode === MATERIAL_ALPHA_MODE.BLEND) {
        this.transparentRanges.push({material: triangle.material, firstIndex: offset, indexCount: 3, depth: depth(triangle.center)});
      }
      const previous = this.runs.at(-1);
      if(previous?.material === triangle.material) {
        previous.indexCount+= 3;
      } else {
        this.runs.push({material: triangle.material, firstIndex: offset, indexCount: 3, transparent: this.definitions[triangle.material].alphaMode === MATERIAL_ALPHA_MODE.BLEND});
      }
      offset+= 3;
    }
    if(indices.length) {
      this.indices?.write(indices);
    }
  }

  /** Draw one depth phase into the borrowed scene pass. @param pass @param transparent */
  draw(pass: RenderPass, transparent: boolean) {
    for(const run of this.runs) {
      if(run.transparent !== transparent) {
        continue;
      }
      this.drawRange(pass, run, transparent);
    }
  }

  /** Draw a prepared index range, allowing the renderer to interleave other materials.
   * @param pass
   * @param range Borrowed; valid until prepare/setMesh.
   * @param transparent Whether this range uses straight alpha and read-only depth.
   */
  drawRange(pass: RenderPass, range: {
    material: number;
    firstIndex: number;
    indexCount: number;
}, transparent: boolean) {
    const {uniforms, ...textures} = this.materials.bindings[range.material];
    pass.setPipeline(transparent ? this.transparentPipeline : this.opaquePipeline);
    pass.setVertexArray(this.vertexArray);
    pass.setBindings({frameUniforms: this.uniforms, materialUniforms: uniforms, ...textures});
    // The WebGL adapter consumes indexed byte offsets through firstVertex;
    // WebGPU uses firstIndex in index units. Keep that backend detail here.
    const offset = this.device.type === GPU_BACKEND.WEBGL
      ? {firstVertex: range.firstIndex*Uint32Array.BYTES_PER_ELEMENT}
      : {firstIndex: range.firstIndex};
    pass.draw({...offset, indexCount: range.indexCount});
  }

  /** Cancel pending decodes and release all owned resources before the device. */
  destroy() {
    if(this.disposed) {
      return;
    }
    this.disposed = true;
    this.revision++;
    this.materials.destroy();
    this.vertices?.destroy();
    this.indices?.destroy();
    this.uniforms.destroy();
    this.vertexArray.destroy();
    this.opaquePipeline.destroy();
    this.transparentPipeline.destroy();
    for(const shader of new Set(Object.values(this.shaders))) {
      shader.destroy();
    }
  }
}

