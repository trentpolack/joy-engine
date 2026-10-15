// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {Buffer} from '@luma.gl/core';
import type {Binding, Device, RenderPass, RenderPipeline, TextureFormatColor} from '@luma.gl/core';
import {getShaderLayoutFromWGSL} from '@luma.gl/webgpu';
import {sceneShadowModule as shadow} from '../shadows/scene-shadow-module.ts';
import {createPortableShaders} from '../gpu/gpu-device.ts';
import {invertMatrix} from '../camera/projection.ts';
import {DEFAULT_SHADOW_OPTIONS} from '../shadows/types.ts';
import type {ShadowOptions} from '../shadows/types.ts';
import type {TerrainAsset} from './terrain-asset.ts';
import type {TerrainView} from './terrain-selection.ts';
import {validateTerrainView} from './terrain-selection.ts';
import {TerrainRenderer} from './terrain-renderer.ts';
import geometry from '../../../shaders/rendering/terrain/terrain-geometry.wgsl?raw';
import lighting from '../../../shaders/rendering/scene/scene-lighting.wgsl?raw';
import sceneSource from '../../../shaders/rendering/terrain/terrain-scene.wgsl?raw';
import casterSource from '../../../shaders/rendering/terrain/terrain-caster.wgsl?raw';
import surfaceSource from '../../../shaders/rendering/terrain/terrain-surface.wgsl?raw';
import motionSource from '../../../shaders/rendering/terrain/terrain-motion.wgsl?raw';
import vertex from '../../../shaders/rendering/terrain/terrain.vert.glsl?raw';
import fragment from '../../../shaders/rendering/terrain/terrain-scene.frag.glsl?raw';

/** Scene adapter owns native GPU resources; scene nodes and heightfield assets remain borrowed. */
export class TerrainScenePass {
  readonly terrain: TerrainRenderer;
  readonly asset: TerrainAsset;
  private readonly model: Buffer;
  private readonly opaque: RenderPipeline;
  private shadowed: RenderPipeline | null = null;
  private additive: RenderPipeline | null = null;
  private caster: RenderPipeline | null = null;
  private surface: RenderPipeline | null = null;
  private motion: RenderPipeline | null = null;
  private surfaceUniforms: Buffer | null = null;
  private motionUniforms: Buffer | null = null;
  private currentModel = new Float32Array(16);
  private previousModel = new Float32Array(16);
  private readonly resources: {destroy(): void}[] = [];
  private disposed = false;

  constructor(device: Device, asset: TerrainAsset, colorFormat?: TextureFormatColor, shadows: ShadowOptions | false = {}) {
    this.asset = asset;
    try {
      this.terrain = this.own(new TerrainRenderer(device, asset, colorFormat));
      this.model = this.own(device.createBuffer({id: 'terrain-world-transform', byteLength: 64, usage: Buffer.UNIFORM | Buffer.COPY_DST}));
      this.opaque = this.createPipeline(device, colorFormat, false, false, shadows || {});
      if(device.type === 'webgpu' && shadows !== false) {
        this.shadowed = this.createPipeline(device, colorFormat, true, false, shadows);
        this.additive = this.own(device.createRenderPipeline({...this.shadowed.props, id: 'terrain-scene-additive', parameters: {
          depthWriteEnabled: false, depthCompare: 'equal', cullMode: 'none', blend: true,
          blendColorOperation: 'add', blendAlphaOperation: 'add', blendColorSrcFactor: 'one', blendColorDstFactor: 'one',
          blendAlphaSrcFactor: 'zero', blendAlphaDstFactor: 'one'
        }}));
        this.caster = this.createPipeline(device, colorFormat, false, true, shadows);
      }
      if(device.type === 'webgpu') {
        this.surface = this.createAttachmentPipeline(device, surfaceSource, ['rgba16float'], 'terrain-surface', 'equal', false);
        this.motion = this.createAttachmentPipeline(device, motionSource, ['rgba16float', 'rgba16float'], 'terrain-motion', 'less-equal', true);
        this.surfaceUniforms = this.own(device.createBuffer({id: 'terrain-surface-uniforms', byteLength: 144, usage: Buffer.UNIFORM | Buffer.COPY_DST}));
        this.motionUniforms = this.own(device.createBuffer({id: 'terrain-motion-uniforms', byteLength: 336, usage: Buffer.UNIFORM | Buffer.COPY_DST}));
      }
    } catch(error) {
      this.destroy();
      throw error;
    }
  }

  /** Prepare one morphed surface for both camera and light views. Inputs use world coordinates. */
  prepare(matrix: ArrayLike<number>, worldTransform: ArrayLike<number>, view: TerrainView, diagnostic: number = 0) {
    if(this.disposed) {
      throw new Error('Terrain scene pass is destroyed.');
    }
    validateTerrainView(view);
    if(worldTransform.length !== 12 || !Array.from(worldTransform).every(Number.isFinite)) {
      throw new RangeError('Terrain requires a finite affine world transform.');
    }
    const model = new Float32Array(16);
    for(let column = 0; column < 4; column++) {
      for(let row = 0; row < 3; row++) {
        model[column*4 + row] = worldTransform[column*3 + row];
      }
    }
    model[15] = 1;
    if(!model.every(Number.isFinite)) {
      throw new RangeError('Terrain world transform exceeds GPU precision.');
    }
    const inverse = invertMatrix(model);
    // The condition bound keeps projected local errors conservative under nonuniform
    // scale and shear. A collapsed transform uses finest geometry without inversion.
    const localView: TerrainView = inverse ? {
      ...view, eye: transformPoint(inverse, view.eye), target: transformPoint(inverse, view.target),
      projectionScale: view.projectionScale*stretchBound(model)*stretchBound(inverse),
      targetRadius: view.targetRadius*stretchBound(inverse)
    } : {...view, eye: [0, 0, 0], target: [0, 0, 0], targetRadius: Math.max(...this.asset.manifest.extent)*2};
    this.terrain.prepare(matrix, localView, diagnostic);
    this.previousModel.set(this.currentModel.some(value => value !== 0) ? this.currentModel : model);
    this.currentModel.set(model);
    this.model.write(model);
  }

  /** Draw native color/depth into the frame owner's pass, using its light snapshots. */
  draw(pass: RenderPass, bindings: Record<string, Binding>, shadowed: boolean, additive: boolean) {
    const pipeline = shadowed ? (additive ? this.additive : this.shadowed) : this.opaque;
    if(!pipeline) {
      throw new Error('Terrain shadow pipeline is unavailable.');
    }
    this.terrain.drawGeometry(pass, pipeline, {...bindings, terrainModel: this.model});
  }

  /** The same indexed/morphed geometry writes the borrowed light-space depth attachment. */
  drawShadow(pass: RenderPass, matrix: Buffer) {
    if(this.caster) {
      this.terrain.drawGeometry(pass, this.caster, {casterCamera: matrix, terrainModel: this.model});
    }
  }

  /** Write view-space normal and roughness for the resident terrain surface. */
  drawSurface(pass: RenderPass, viewProjection: ArrayLike<number>, viewMatrix: ArrayLike<number>) {
    if(!this.surface || !this.surfaceUniforms) {
      return;
    }
    const data = new Float32Array(36);
    data.set(Array.from(viewProjection), 0);
    data.set(Array.from(viewMatrix), 16);
    data[32] = 1;
    this.surfaceUniforms.write(data);
    this.terrain.drawGeometry(pass, this.surface, {attachment: this.surfaceUniforms, terrainModel: this.model});
  }

  /** Write static-terrain camera and transform motion without expanding the heightfield. */
  drawMotion(pass: RenderPass, frame: {
    currentViewProjection: ArrayLike<number>;
    previousViewProjection: ArrayLike<number>;
    rasterViewProjection: ArrayLike<number>;
    velocityJitterDelta: readonly [number, number];
  }) {
    if(!this.motion || !this.motionUniforms) {
      return;
    }
    const data = new Float32Array(84);
    data.set(Array.from(frame.currentViewProjection), 0);
    data.set(Array.from(frame.previousViewProjection), 16);
    data.set(Array.from(frame.rasterViewProjection), 32);
    data.set(this.currentModel, 48);
    data.set(this.previousModel, 64);
    data.set(frame.velocityJitterDelta, 80);
    this.motionUniforms.write(data);
    this.terrain.drawGeometry(pass, this.motion, {attachment: this.motionUniforms});
  }

  destroy() {
    if(this.disposed) {
      return;
    }
    this.disposed = true;
    for(const resource of this.resources.splice(0).reverse()) {
      resource.destroy();
    }
  }

  private own<T extends {destroy(): void}>(resource: T): T {
    this.resources.push(resource);
    return resource;
  }

  private createPipeline(device: Device, colorFormat: TextureFormatColor | undefined, shadowed: boolean, caster: boolean, options: ShadowOptions) {
    const shaders = createPortableShaders(device, {
      wgsl: geometry + (caster ? casterSource : lighting + sceneSource), glsl: {vertex, fragment},
      modules: shadowed ? [shadow] : [], defines: {JOY_SCENE: true, ...(shadowed ? {JOY_SHADOWS: true} : {})}
    }, caster ? 'terrain-caster' : 'terrain-scene');
    this.resources.push(...new Set(Object.values(shaders)));
    const shaderLayout = device.type === 'webgpu' ? getShaderLayoutFromWGSL(shaders.vertex.source, {vertexEntryPoint: 'vertexMain'}) : undefined;
    if(shaderLayout) {
      shaderLayout.bindings = shaderLayout.bindings.map(binding => binding.name === 'heightTexture' && binding.type === 'texture'
        ? {...binding, sampleType: 'unfilterable-float'} : binding);
    }
    return this.own(device.createRenderPipeline({
      id: caster ? 'terrain-scene-caster' : (shadowed ? 'terrain-scene-shadowed' : 'terrain-scene-opaque'),
      vs: shaders.vertex, fs: shaders.fragment, vertexEntryPoint: 'vertexMain', fragmentEntryPoint: 'fragmentMain',
      ...(shaderLayout ? {shaderLayout} : {}), bufferLayout: this.terrain.pipeline.bufferLayout,
      topology: 'triangle-list', depthStencilAttachmentFormat: caster ? 'depth32float' : 'depth24plus',
      ...(caster ? {colorAttachmentFormats: []} : (colorFormat ? {colorAttachmentFormats: [colorFormat]} : {})),
      parameters: {depthWriteEnabled: true, depthCompare: 'less-equal', cullMode: 'none',
        ...(caster ? {depthBias: options.depthBias ?? DEFAULT_SHADOW_OPTIONS.depthBias,
          depthBiasSlopeScale: options.depthBiasSlopeScale ?? DEFAULT_SHADOW_OPTIONS.depthBiasSlopeScale} : {})}
    }));
  }

  private createAttachmentPipeline(
    device: Device,
    source: string,
    colorAttachmentFormats: TextureFormatColor[],
    id: string,
    depthCompare: 'equal' | 'less-equal',
    depthWriteEnabled: boolean): RenderPipeline {
    const shaders = createPortableShaders(device, {
      wgsl: geometry + source,
      glsl: {vertex: '', fragment: ''},
      defines: {JOY_SCENE: true}
    }, id);
    this.resources.push(...new Set(Object.values(shaders)));
    const shaderLayout = getShaderLayoutFromWGSL(shaders.vertex.source, {vertexEntryPoint: 'vertexMain'});
    if(!shaderLayout) {
      throw new Error(`Terrain attachment shader layout unavailable for ${id}.`);
    }
    shaderLayout.bindings = shaderLayout.bindings.map(binding => binding.name === 'heightTexture' && binding.type === 'texture'
      ? {...binding, sampleType: 'unfilterable-float'} : binding);
    return this.own(device.createRenderPipeline({
      id,
      vs: shaders.vertex,
      fs: shaders.fragment,
      vertexEntryPoint: 'vertexMain',
      fragmentEntryPoint: 'fragmentMain',
      shaderLayout,
      bufferLayout: this.terrain.pipeline.bufferLayout,
      topology: 'triangle-list',
      depthStencilAttachmentFormat: 'depth24plus',
      colorAttachmentFormats,
      parameters: {depthWriteEnabled, depthCompare, cullMode: 'none'}
    }));
  }
}

function transformPoint(matrix: ArrayLike<number>, point: readonly number[]): number[] {
  return [0, 1, 2].map(row => matrix[row]*point[0] + matrix[row + 4]*point[1] + matrix[row + 8]*point[2] + matrix[row + 12]);
}

/** sqrt(||A||1 * ||A||infinity) bounds the maximum affine stretch; translation is excluded. */
function stretchBound(matrix: ArrayLike<number>): number {
  let rows = 0;
  let columns = 0;
  for(let axis = 0; axis < 3; axis++) {
    rows = Math.max(rows, Math.abs(matrix[axis]) + Math.abs(matrix[axis + 4]) + Math.abs(matrix[axis + 8]));
    columns = Math.max(columns, Math.abs(matrix[axis*4]) + Math.abs(matrix[axis*4 + 1]) + Math.abs(matrix[axis*4 + 2]));
  }
  return Math.sqrt(rows*columns);
}
