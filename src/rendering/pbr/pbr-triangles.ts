// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {ibl} from '@luma.gl/shadertools';
import {shadow} from '@luma.gl/experimental';
import {SceneShadows} from '../shadows/scene-shadows.ts';
import {ShadowCasters} from '../shadows/shadow-casters.ts';
import type {ShadowOptions} from '../shadows/types.ts';
import {prepareShadowLightBatches} from '../shadows/shadow-light-batches.ts';
import {DEFAULT_SHADOW_OPTIONS} from '../shadows/types.ts';
import casterWgsl from '../../../shaders/rendering/shadows/pbr-caster.wgsl?raw';
import { GPU_BACKEND, GPU_COMPARE, GPU_TEXTURE_FORMAT, GPU_TOPOLOGY, GPU_VERTEX_FORMAT, SHADER_ENTRY_POINT } from '../gpu/constants.ts';
import { MATERIAL_ALPHA_MODE } from './constants.ts';
import type { TextureFormatColor, RenderPipelineProps, RenderPass, Device, Binding, Shader, RenderPipeline, VertexArray } from '@luma.gl/core';
import type { PbrMesh, PbrFrame, PbrMaterial } from '../../core/types.ts';
import { Buffer, Texture } from '@luma.gl/core';
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
  private declare pipelineOptions: RenderPipelineProps;
  private declare fallbackEnvironment: Texture;
  private readonly frameBuffers: Buffer[] = [];
  private readonly lightingBuffers: Buffer[] = [];
  private activeBindings: Record<string, Binding>[] = [];
  private environmentBindings: Record<string, Binding> = {};
  private shadowResources: {owner: SceneShadows; casters: ShadowCasters; shaders: ReturnType<typeof createPortableShaders>;
    opaque: RenderPipeline; transparent: RenderPipeline; additive: RenderPipeline; transparentAdditive: RenderPipeline;
    casterShader: ReturnType<typeof createPortableShaders>; casterPipeline: RenderPipeline; optionsKey: string} | null = null;
  private activeShadows = false;


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
      this.shaders = createPortableShaders(device, {wgsl, glsl: {vertex, fragment}, modules: [ibl], defines: {USE_IBL: true}}, 'pbr-triangles');
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
      this.pipelineOptions = options;
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
      allocated.push(this.uniforms);
      this.fallbackEnvironment = device.createTexture({id: 'pbr-empty-environment', dimension: 'cube', depth: 6,
        width: 1, height: 1, format: 'rgba8unorm', usage: Texture.SAMPLE,
        sampler: {minFilter: 'linear', magFilter: 'linear'}});

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
  prepare(frame: PbrFrame, includeTransparentRanges: boolean = false, coloredCasters: number[] | Float32Array = []) {
    const values = new Float32Array(32);
    values.set(frame.viewProjection);
    values.set(frame.eye, 16);
    values.set(frame.lightDirection ?? DEFAULT_PBR_LIGHTING.lightDirection, 20);
    values.set(frame.lightColor ?? DEFAULT_PBR_LIGHTING.lightColor, 24);
    values.set(frame.ambientColor ?? DEFAULT_PBR_LIGHTING.ambientColor, 28);
    values[31] = this.displayResolve ? 1 : 0;
    this.uniforms.write(values);
    this.prepareLighting(frame, values);
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
    if(this.activeShadows && this.shadowResources && frame.shadowCamera) {
      this.shadowResources.casters.prepare(coloredCasters);
      const direction = frame.lightDirection ?? DEFAULT_PBR_LIGHTING.lightDirection;
      const color = frame.lightColor ?? DEFAULT_PBR_LIGHTING.lightColor;
      const batches = this.shadowResources.owner.prepare(frame.shadowCamera, frame.pointLights ?? [], {
        direction: [direction[0], direction[1], direction[2]], color: [1, 1, 1], intensity: Math.max(...color),
        kind: 'sun', angularRadius: 0.00465, castsShadow: frame.castsShadow
      }, view => this.shadowResources?.casters.draw(view, (pass, matrix) => this.drawShadow(pass, matrix)));
      this.activeBindings = batches.map(batch => batch.bindings);
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
    if(transparent && this.activeBindings.length > 1 && range.indexCount > 3) {
      // Complete every light contribution before the next triangle's alpha compositing.
      for(let firstIndex = range.firstIndex; firstIndex < range.firstIndex + range.indexCount; firstIndex+= 3) {
        this.drawRange(pass, {...range, firstIndex, indexCount: 3}, true);
      }
      return;
    }
    const {uniforms, ...textures} = this.materials.bindings[range.material];
    const resources = this.activeShadows ? this.shadowResources : null;
    pass.setVertexArray(this.vertexArray);
    const offset = this.device.type === GPU_BACKEND.WEBGL
      ? {firstVertex: range.firstIndex*Uint32Array.BYTES_PER_ELEMENT}
      : {firstIndex: range.firstIndex};
    // Finish this surface's direct-light groups before compositing the next transparent surface.
    for(let batch = 0; batch < this.lightingBuffers.length && batch < this.activeBindings.length; batch++) {
      const basePipeline = transparent ? this.transparentPipeline : this.opaquePipeline;
      const pipeline = resources ? (batch === 0 ? (transparent ? resources.transparent : resources.opaque)
        : (transparent ? resources.transparentAdditive : resources.additive)) : basePipeline;
      pass.setPipeline(pipeline);
      pass.setBindings({frameUniforms: batch === 0 ? this.uniforms : this.frameBuffers[batch - 1],
        materialUniforms: uniforms, pbrLighting: this.lightingBuffers[batch], ...textures,
        ...this.environmentBindings, ...this.activeBindings[batch]});
      pass.draw({...offset, indexCount: range.indexCount});
    }
  }

  /** Cancel pending decodes and release all owned resources before the device. */
  destroy() {
    if(this.disposed) {
      return;
    }
    this.disposed = true;
    this.revision++;
    this.releaseShadows();
    this.fallbackEnvironment.destroy();
    for(const buffer of [...this.lightingBuffers, ...this.frameBuffers]) {
      buffer.destroy();
    }
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

  private prepareLighting(frame: PbrFrame, baseValues: Float32Array) {
    const environment = frame.environment?.bindings;
    this.environmentBindings = {
      pbr_diffuseEnvSampler: environment?.diffuseTexture ?? this.fallbackEnvironment,
      pbr_specularEnvSampler: environment?.specularTexture ?? this.fallbackEnvironment,
      pbr_brdfLUT: environment?.brdfLUTTexture ?? this.materials.whiteTexture
    };
    const intensity = frame.environment?.intensity ?? 0;
    const rotation = frame.environment?.rotation ?? 0;
    if(!Number.isFinite(intensity) || intensity < 0 || !Number.isFinite(rotation)) {
      throw new RangeError('Environment intensity must be nonnegative and rotation finite.');
    }
    const lights = frame.pointLights ?? [];
    prepareShadowLightBatches(lights);
    this.activeShadows = Boolean(frame.shadowCamera && frame.shadows !== false && this.device.type === GPU_BACKEND.WEBGPU);
    if(this.activeShadows) {
      this.ensureShadows(frame.shadows || {});
    }
    const groups = this.activeShadows ? prepareShadowLightBatches(lights) : [{lights, shadowIndices: lights.map(() => -1)}];
    this.activeBindings = groups.map(() => ({}));
    for(const [index, group] of groups.entries()) {
      const lighting = new Float32Array(280);
      if(frame.shadowCamera) {
        lighting.set(Array.from(frame.shadowCamera.viewMatrix), 0);
      }
      lighting.set([index === 0 && environment ? intensity : 0, rotation, (environment?.specularTexture.mipLevels ?? 1) - 1, 0], 16);
      group.lights.forEach((light, slot) => {
        if(![light.x, light.y, light.z, light.r, light.g, light.b, light.range].every(Number.isFinite) || light.range <= 0 || Math.min(light.r, light.g, light.b) < 0) {
          throw new RangeError('PBR point lights require finite positions/colors and positive ranges.');
        }
        lighting.set([light.x, light.y, light.z, light.range, light.r, light.g, light.b, group.shadowIndices[slot] + 1], 20 + slot*8);
      });
      lighting.set([group.lights.length, index === 0 ? 1 : 0, 0, 0], 276);
      if(!this.lightingBuffers[index]) {
        this.lightingBuffers.push(this.device.createBuffer({id: 'pbr-lighting', byteLength: lighting.byteLength, usage: Buffer.UNIFORM | Buffer.COPY_DST}));
      }
      this.lightingBuffers[index].write(lighting);
      if(index > 0) {
        const values = baseValues.slice();
        values.fill(0, 24, 31);
        values[31] = 0;
        if(!this.frameBuffers[index - 1]) {
          this.frameBuffers.push(this.device.createBuffer({id: 'pbr-additive-frame', byteLength: 128, usage: Buffer.UNIFORM | Buffer.COPY_DST}));
        }
        this.frameBuffers[index - 1].write(values);
      }
    }
  }

  /** Copy per-instance shadow work without exposing owned GPU resources. */
  get shadowStats() {
    return this.activeShadows && this.shadowResources ? {...this.shadowResources.owner.stats} : null;
  }

  private ensureShadows(options: ShadowOptions) {
    // Receiver bias and fitted distance change with inspection zoom, without
    // changing resource shape. Retain maps, shaders, and pipelines across these edits.
    const {normalBias, shadowDistance, ...allocationOptions} = options;
    const optionsKey = JSON.stringify(allocationOptions);
    if(this.shadowResources?.optionsKey === optionsKey) {
      this.shadowResources.owner.setReceiverOptions({normalBias, shadowDistance});
      return;
    }
    const allocated: {destroy(): void}[] = [];
    try {
      const owner = new SceneShadows(this.device, options);
      allocated.push(owner);
      const casters = new ShadowCasters(this.device, options);
      allocated.push(casters);
      const shaders = createPortableShaders(this.device, {wgsl, glsl: {vertex, fragment}, modules: [ibl, shadow],
        defines: {USE_IBL: true, JOY_SHADOWS: true}}, 'pbr-shadow');
      allocated.push(shaders.vertex);
      const common = {...this.pipelineOptions, vs: shaders.vertex, fs: shaders.fragment};
      const opaque = this.device.createRenderPipeline({...common, id: 'pbr-shadow-base', parameters: this.opaquePipeline.props.parameters});
      allocated.push(opaque);
      const transparent = this.device.createRenderPipeline({...common, id: 'pbr-shadow-transparent', parameters: this.transparentPipeline.props.parameters});
      allocated.push(transparent);
      const additiveParameters = {depthWriteEnabled: false, depthCompare: 'equal' as const, cullMode: 'none' as const, blend: true,
        blendColorOperation: 'add' as const, blendAlphaOperation: 'add' as const,
        blendColorSrcFactor: 'one' as const, blendColorDstFactor: 'one' as const,
        blendAlphaSrcFactor: 'zero' as const, blendAlphaDstFactor: 'one' as const};
      const additive = this.device.createRenderPipeline({...common, id: 'pbr-shadow-additive', parameters: additiveParameters});
      allocated.push(additive);
      const transparentAdditive = this.device.createRenderPipeline({...common, id: 'pbr-shadow-transparent-additive',
        parameters: {...additiveParameters, depthCompare: 'less-equal', blendColorSrcFactor: 'src-alpha'}});
      allocated.push(transparentAdditive);
      const casterShader = createPortableShaders(this.device, {wgsl: casterWgsl, glsl: {vertex: '', fragment: ''}}, 'pbr-caster');
      allocated.push(casterShader.vertex);
      const casterPipeline = this.device.createRenderPipeline({vs: casterShader.vertex, fs: casterShader.fragment,
        vertexEntryPoint: 'vertexMain', fragmentEntryPoint: 'fragmentMain', topology: 'triangle-list',
        bufferLayout: this.pipelineOptions.bufferLayout, depthStencilAttachmentFormat: 'depth32float', colorAttachmentFormats: [],
        parameters: {depthWriteEnabled: true, depthCompare: 'less-equal', cullMode: 'none',
          depthBias: options.depthBias ?? DEFAULT_SHADOW_OPTIONS.depthBias,
          depthBiasSlopeScale: options.depthBiasSlopeScale ?? DEFAULT_SHADOW_OPTIONS.depthBiasSlopeScale}});
      allocated.push(casterPipeline);
      this.releaseShadows();
      this.shadowResources = {owner, casters, shaders, opaque, transparent, additive, transparentAdditive, casterShader, casterPipeline, optionsKey};
    } catch(error) {
      for(const resource of allocated.reverse()) {
        resource.destroy();
      }
      throw error;
    }
  }

  /** Record material casters into a borrowed colored-scene light view using its immutable camera snapshot. */
  drawShadow(pass: RenderPass, matrix: Buffer) {
    const resources = this.shadowResources;
    if(!resources) {
      return;
    }
    pass.setPipeline(resources.casterPipeline);
    pass.setVertexArray(this.vertexArray);
    for(const run of this.runs) {
      if(run.transparent) {
        continue;
      }
      const binding = this.materials.bindings[run.material];
      pass.setBindings({casterCamera: matrix, materialUniforms: binding.uniforms, baseColorTexture: binding.baseColorTexture});
      pass.draw({firstIndex: run.firstIndex, indexCount: run.indexCount});
    }
  }

  private releaseShadows() {
    const resources = this.shadowResources;
    if(!resources) {
      return;
    }
    resources.owner.destroy();
    resources.casters.destroy();
    resources.shaders.vertex.destroy();
    resources.casterShader.vertex.destroy();
    resources.casterPipeline.destroy();
    resources.opaque.destroy();
    resources.transparent.destroy();
    resources.additive.destroy();
    resources.transparentAdditive.destroy();
    this.shadowResources = null;
  }

}

