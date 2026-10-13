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
