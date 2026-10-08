// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { GPU_BACKEND, GPU_COMPARE, GPU_TEXTURE_FORMAT, GPU_TOPOLOGY, GPU_VERTEX_FORMAT, SHADER_ENTRY_POINT } from '../gpu/constants.ts';
import type { RendererDiagnosticOptions, RendererDiagnostics } from '../gpu/gpu-diagnostics.ts';
import { readGpuDiagnostics } from '../gpu/gpu-diagnostics.ts';
import type { SceneEffectsOptions, SceneEffectsFrame } from '../scene/scene-effects.ts';
import type { GpuDeviceOptions } from '../gpu/gpu-device.ts';
import type { RenderPipelineProps, Framebuffer, Texture, Device, CanvasContext, Shader, RenderPipeline, VertexArray } from '@luma.gl/core';
import type { PbrMesh, RgbaColor, PbrFrame } from '../../core/types.ts';
import type { TransparentDraw } from './textured-triangles.ts';
import type { PbrTransparentRange } from '../pbr/pbr-triangles.ts';
import type { PostProcessingConfig } from '../postprocessor/config.ts';
import { AtmospherePass } from '../environment/atmosphere-pass.ts';
import { PbrTriangles } from '../pbr/pbr-triangles.ts';
import { TexturedTriangles } from './textured-triangles.ts';
import { Postprocessor } from '../postprocessor/postprocessor.ts';
import type {SceneAttachmentFrame} from '../scene/scene-effect-attachments.ts';
import {SceneEffectAttachments} from '../scene/scene-effect-attachments.ts';
import {getSceneEffectsRequirements} from '../scene/scene-effect-config.ts';
import { SceneEffects } from '../scene/scene-effects.ts';
import { Buffer } from '@luma.gl/core';
import { DynamicGpuBuffer } from '../gpu/dynamic-gpu-buffer.ts';
import { createGpuCanvasDevice, createPortableShaders } from '../gpu/gpu-device.ts';
import { FLOATS_PER_VERTEX } from '../geometry/geometry.ts';
import { addPostProcessingShaders, createPostProcessingUniforms, normalizePostProcessingConfig } from '../postprocessor/config.ts';

export interface TriangleShaderSources {
  wgsl: string;
  glsl: {
      vertex: string;
      fragment: string;
  };
}
export interface RendererOptions {
  shaderSources: TriangleShaderSources;
  texturedShaderSources?: TriangleShaderSources;
  opaqueShaderSources?: TriangleShaderSources;
  uniformBufferSize?: number;
  initialVertexBufferSize?: number;
  pbr?: boolean;
  atmosphere?: boolean;
  postProcessing?: Partial<PostProcessingConfig>;
  sceneEffects?: SceneEffectsOptions;
  device?: GpuDeviceOptions;
}

/**
 * Depth-tested 3D triangle rendering through luma.gl's portable Device API.
 * Games provide world-space vertices, camera uniforms, and matching shaders.
 *
 * Opaque triangles write depth. Translucent triangles use straight-alpha
 * source-over blending and test against opaque depth without writing it.
 * The caller supplies translucent triangles already sorted back-to-front for
 * its camera. Both batches share one framebuffer and one clear per frame.
 */
export class GpuTriangleRenderer {
  declare device: Device;
  declare canvasContext: CanvasContext;
  declare backend: typeof GPU_BACKEND.WEBGL | typeof GPU_BACKEND.WEBGPU | "null" | "unknown";
  declare width: number;
  declare height: number;
  declare drawingBufferWidth: number;
  declare drawingBufferHeight: number;
  declare sceneUniformBufferSize: number;
  declare postProcessing: PostProcessingConfig | null;
  declare postprocessor: Postprocessor | null;
  declare sceneEffects: SceneEffects | null;
  private sceneAttachments: SceneEffectAttachments | null = null;
  private compositeFramebuffer: Framebuffer | null = null;
  private compositeTexture: Texture | null = null;
  private compositeDepth: Texture | null = null;
  declare atmosphere: AtmospherePass | null;
  declare resourcesReleased: boolean;
  declare destroyed: boolean;
  declare pbr: PbrTriangles | null;
  declare textured: TexturedTriangles | null;
  declare opaqueShaders: { vertex: Shader; fragment: Shader; } | null;
  declare shaders: ReturnType<typeof createPortableShaders>;
  declare pipeline: RenderPipeline;
  declare transparentPipeline: RenderPipeline;
  declare vertexArray: VertexArray;
  declare uniformBuffer: Buffer;
  declare vertexBuffer: DynamicGpuBuffer;

  /**
   * @param canvas Canvas dedicated to GPU rendering.
   * @param options Backend-specific shader sources.
   */
  static async create(canvas: HTMLCanvasElement, options: RendererOptions): Promise<GpuTriangleRenderer> {
    const device = await createGpuCanvasDevice(canvas, options.device);
    const canvasContext = ((device.canvasContext) as CanvasContext);
    try {
      return(new GpuTriangleRenderer(device, canvasContext, options));
    } catch(error) {
      canvasContext.destroy();
      device.destroy();
      throw error;
    }
  }

  /**
   * @param device
   * @param canvasContext
   * @param options
   */
  constructor(device: Device, canvasContext: CanvasContext, { shaderSources, texturedShaderSources, opaqueShaderSources, uniformBufferSize = 16, initialVertexBufferSize = 64*1024, postProcessing, sceneEffects, pbr = false, atmosphere = false }: RendererOptions) {
    if(sceneEffects && !postProcessing) {
      throw new Error('Scene effects require an offscreen post-processing scene.');
    }
    // Ownership transfers to this renderer; dispose passes and buffers before these roots.
    this.device = device;
    this.canvasContext = canvasContext;
    this.backend = device.type;
    this.width = 0;
    this.height = 0;
    this.drawingBufferWidth = 0;
    this.drawingBufferHeight = 0;

    // Optional scene stages own their resources and borrow the shared device.
    this.sceneUniformBufferSize = uniformBufferSize;
    this.postProcessing = postProcessing ? normalizePostProcessingConfig(postProcessing) : null;
    this.postprocessor = this.postProcessing ? new Postprocessor(device) : null;
    if(sceneEffects?.colorFormat && sceneEffects.colorFormat !== this.postprocessor?.format) {
      throw new Error(`Scene effects colorFormat must match the post-processing scene format ${this.postprocessor?.format}.`);
    }
    this.sceneEffects = sceneEffects ? new SceneEffects(device, {
      ...sceneEffects,
      colorFormat: this.postprocessor?.format
    }) : null;

    this.atmosphere = null;
    this.resourcesReleased = false;
    this.destroyed = false;
    this.pbr = pbr ? new PbrTriangles(device, this.postprocessor?.format) : null;
    // Templates need a fragment output even when this renderer draws directly to
    // the canvas. Fully authored caller shaders require no template expansion.
    const needsShaderExpansion = this.postProcessing || shaderSources.wgsl?.includes('JOY_POST_APPLY_MACRO');
    const resolvedShaderSources = needsShaderExpansion
      ? addPostProcessingShaders(shaderSources, {includeUniforms: Boolean(this.postProcessing)})
      : shaderSources;
    this.textured = texturedShaderSources ? new TexturedTriangles(
      device, texturedShaderSources,
      this.postprocessor?.format,
    ) : null;

    this.opaqueShaders = opaqueShaderSources ? createPortableShaders(device, opaqueShaderSources, 'lit-opaque-triangles') : null;
    this.shaders = createPortableShaders(device, resolvedShaderSources, 'triangle-renderer');

    const pipelineOptions: RenderPipelineProps = {
      vs: this.shaders.vertex,
      fs: this.shaders.fragment,
      vertexEntryPoint: SHADER_ENTRY_POINT.VERTEX,
      fragmentEntryPoint: SHADER_ENTRY_POINT.FRAGMENT,
      topology: GPU_TOPOLOGY.TRIANGLE_LIST,
      bufferLayout: [{
        name: 'vertices',
        byteStride: FLOATS_PER_VERTEX*Float32Array.BYTES_PER_ELEMENT,
        attributes: [
          { attribute: 'position', format: GPU_VERTEX_FORMAT.FLOAT32X3, byteOffset: 0 },
          { attribute: 'color', format: GPU_VERTEX_FORMAT.FLOAT32X4, byteOffset: 12 }
        ]
      }],
      // Explicit format is required even when a pipeline only reads depth.
      depthStencilAttachmentFormat: GPU_TEXTURE_FORMAT.DEPTH24PLUS,
      ...(this.postprocessor ? { colorAttachmentFormats: [this.postprocessor.format] } : {})
    };
    this.pipeline = device.createRenderPipeline({
      id: 'triangle-opaque-pipeline',
      ...pipelineOptions,
      ...(this.opaqueShaders ? { vs: this.opaqueShaders.vertex, fs: this.opaqueShaders.fragment } : {}),
      parameters: {
        depthWriteEnabled: true,
        depthCompare: GPU_COMPARE.LESS_EQUAL,
        blend: false
      }
    });
    this.transparentPipeline = device.createRenderPipeline({
      id: 'triangle-transparent-pipeline',
      ...pipelineOptions,
      parameters: {
        depthWriteEnabled: false,
        depthCompare: GPU_COMPARE.LESS_EQUAL,
        blend: true,
        // Specify operations as well as factors: luma.gl's WebGL adapter applies
        // the blend functions when an operation is present in the parameters.
        blendColorOperation: 'add',
        blendAlphaOperation: 'add',
        blendColorSrcFactor: 'src-alpha',
        blendColorDstFactor: 'one-minus-src-alpha',
        blendAlphaSrcFactor: 'one',
        blendAlphaDstFactor: 'one-minus-src-alpha'
      }
    });
    this.vertexArray = device.createVertexArray({
      id: 'triangle-vertex-array',
      shaderLayout: this.pipeline.shaderLayout,
      bufferLayout: this.pipeline.bufferLayout
    });
    // Derive the allocation from the packed shader tail so new effects cannot
    // make scene uploads exceed the GPU buffer's capacity.
    const postProcessingUniformBytes = this.postProcessing ? createPostProcessingUniforms(this.postProcessing, 0, 0, 0).byteLength : 0;
    this.uniformBuffer = device.createBuffer({
      id: 'triangle-uniforms',
      byteLength: uniformBufferSize + postProcessingUniformBytes,
      usage: Buffer.UNIFORM | Buffer.COPY_DST
    });
    this.vertexBuffer = new DynamicGpuBuffer(device, {
      id: 'triangle-vertices',
      usage: Buffer.VERTEX | Buffer.COPY_DST,
      initialByteLength: initialVertexBufferSize,
      onReplace: buffer => this.bindVertexBuffer(buffer)
    });
    // Create the optional background after core geometry setup. Its rollback
    // releases its own partial allocations; the factory releases device roots.
    if(atmosphere) {
      try {
        this.atmosphere = new AtmospherePass(device, this.postprocessor?.format);
      } catch(error) {
        this.releaseResources();
        throw error;
      }
    }
  }

  /**
   * Copy requested luma counters on demand. Scope is all luma devices in this realm,
   * not this renderer alone. Returns null after disposal; performs no GPU readback.
   */
  getDiagnostics(options: RendererDiagnosticOptions = {}): RendererDiagnostics | null {
    return this.destroyed ? null : readGpuDiagnostics(this.device, options);
  }

  /** Upload a world-space material mesh; await readiness before displaying new document data.
   * Requires pbr at creation. The renderer owns buffers/textures and copies input data.
   * @param mesh */
  async setMaterialMesh(mesh: PbrMesh): Promise<boolean> {
    if(!this.pbr) {
      throw new Error('Material meshes require pbr at renderer creation.');
    }
    return this.pbr.setMesh(mesh);
  }

  /**
   * Replace authored settings for the next frame without reallocating GPU resources.
   * Requires postProcessing at creation; omitted fields reset to engine defaults.
   * Invalid settings throw before modifying the last working configuration.
   * @param config Borrowed settings, copied on success.
   */
  setPostProcessingConfig(config: Partial<PostProcessingConfig>) {
    if(!this.postprocessor) {
      throw new Error('Live post-processing requires postProcessing at renderer creation.');
    }
    this.postProcessing = normalizePostProcessingConfig(config);
  }

  /** Replace the optional scene graph atomically; failed configuration retains the previous graph. */
  setSceneEffectsConfig(config: SceneEffectsOptions) {
    if(this.destroyed || !this.postprocessor) {
      throw new Error('Scene effects require a live offscreen scene renderer.');
    }
    const replacement = new SceneEffects(this.device, {...config, colorFormat: this.postprocessor.format});
    this.destroyCompositeFramebuffer();
    this.sceneEffects?.destroy();
    this.sceneEffects = replacement;
    this.sceneAttachments?.destroy();
    this.sceneAttachments = null;
    this.destroyCompositeFramebuffer();
  }

  resetSceneHistory() {
    this.sceneEffects?.resetHistory();
  }

  get sceneVolumeStats() {
    return this.sceneAttachments?.volumeStats ?? null;
  }

  /**
   * Bind the interleaved stream using each backend's vertex-array semantics.
   * @param [buffer]
   */
  bindVertexBuffer(buffer: Buffer = this.vertexBuffer?.buffer) {
    // WebGPU binds a buffer slot; WebGL binds individual attribute locations.
    // Both attributes must be rebound when the shared allocation grows.
    if(!buffer) {
      return;
    }

    this.vertexArray.setBuffer(0, buffer);

    if(this.backend === GPU_BACKEND.WEBGL) {
      this.vertexArray.setBuffer(1, buffer);
    }
  }

  /**
   * Resize the backing surface without changing the caller's logical geometry.
   * @param width Logical width in CSS pixels.
   * @param height Logical height in CSS pixels.
   * @param pixelRatio Backing pixels per CSS pixel.
   */
  resize(width: number, height: number, pixelRatio: number) {
    this.width = width;
    this.height = height;
    this.drawingBufferWidth = Math.max(1, Math.round(width*pixelRatio));
    this.drawingBufferHeight = Math.max(1, Math.round(height*pixelRatio));
    this.canvasContext.setDrawingBufferSize(this.drawingBufferWidth, this.drawingBufferHeight);
    this.postprocessor?.resize(this.drawingBufferWidth, this.drawingBufferHeight, width, height);
  }

  /**
   * Submit opaque geometry, then sorted translucent geometry; clear even if empty.
   * @param vertices Triangle list of x, y, z, r, g, b, a values.
   * @param uniformData Caller-defined values fitting the configured uniform buffer.
   * @param clearColor Frame background.
   * @param transparentVertices Back-to-front x/y/z/RGBA triangles.
   * @param timeSeconds Animation time used by temporal post effects.
   * @param transparentDraws
   * Additional globally sorted material runs (XYZ/RGBA, or XYZ/RGBA/UV with texture).
   * Borrowed textures must outlive submission. Requires texturedShaderSources for texture runs.
   * @param materialFrame Camera/lighting for the optional PBR mesh. Null hides it.
   * @param [sceneFrame] Borrowed motion/camera inputs for requested scene effects. Depth defaults to this scene's owned attachment. Callers must supply real velocities and projection jitter; reset history after camera cuts or scene replacement.
   * @param [atmosphereUniforms] Optional packed background snapshot. Requires atmosphere at creation; null hides it.
   */
  render(vertices: number[] | Float32Array, uniformData: Float32Array<ArrayBuffer>, clearColor: RgbaColor, transparentVertices: number[] | Float32Array = [], timeSeconds: number = 0, transparentDraws: readonly TransparentDraw[] = [], materialFrame: PbrFrame | null = null, sceneFrame: SceneEffectsFrame = {}, atmosphereUniforms: Float32Array<ArrayBuffer> | null = null, attachmentFrame: SceneAttachmentFrame | null = null) {
    if(atmosphereUniforms && !this.atmosphere) {
      throw new Error('Atmosphere uniforms require atmosphere at renderer creation.');
    }
    // Upload once; draw ranges select the two materials without reallocating
    // buffers or resetting depth between the opaque and translucent stages.
    if(!this.textured && transparentDraws.some(draw => draw.texture)) {
      throw new Error('Textured draws require texturedShaderSources at renderer creation.');
    }
    const additionalLength = transparentDraws.reduce((total, draw) => total + (draw.texture ? 0 : draw.vertices.length), 0);
    const vertexData = new Float32Array(vertices.length + transparentVertices.length + additionalLength);
    vertexData.set(vertices);
    vertexData.set(transparentVertices, vertices.length);
    let coloredOffset = vertices.length + transparentVertices.length;
    for(const draw of transparentDraws) {
      if(!draw.texture) {
        vertexData.set(draw.vertices, coloredOffset);
        coloredOffset+= draw.vertices.length;
      }
    }
    this.textured?.prepare(transparentDraws);

    if(vertexData.length > 0) {
      this.vertexBuffer.write(vertexData);
    }
    const sceneUniformBufferSize = this.sceneUniformBufferSize ?? uniformData.byteLength;
    if(uniformData.byteLength !== sceneUniformBufferSize) {
      throw new RangeError(`Expected ${sceneUniformBufferSize} bytes of scene uniforms, received ${uniformData.byteLength}.`);
    }
    if(this.postProcessing) {
      const postProcessingUniforms = createPostProcessingUniforms(
        this.postProcessing,
        this.drawingBufferWidth,
        this.drawingBufferHeight,
        timeSeconds,
      );
      const combinedUniforms = new Float32Array(uniformData.length + postProcessingUniforms.length);
      combinedUniforms.set(uniformData);
      combinedUniforms.set(postProcessingUniforms, uniformData.length);
      this.uniformBuffer.write(combinedUniforms);
    } else {
      this.uniformBuffer.write(uniformData);
    }

    // Merge material streams only when PBR and caller geometry share the transparent phase.
    const mixedTransparency = Boolean(materialFrame && (transparentVertices.length || transparentDraws.length));
    if(materialFrame) {
      if(!this.pbr) {
        throw new Error('Material frames require pbr at renderer creation.');
      }
      this.pbr.prepare(materialFrame, mixedTransparency);
    }
    let processedScene: Texture | undefined;
    let renderPass = this.device.beginRenderPass({
      // CanvasContext owns and resizes the depth attachment with the surface.
      framebuffer: this.postprocessor?.sceneFramebuffer ?? this.canvasContext.getCurrentFramebuffer({ depthStencilFormat: GPU_TEXTURE_FORMAT.DEPTH24PLUS }),
      clearColor: [clearColor[0], clearColor[1], clearColor[2], clearColor[3]],
      clearDepth: 1
    });
    if(atmosphereUniforms) {
      this.atmosphere?.draw(renderPass, atmosphereUniforms);
    }
    renderPass.setPipeline(this.pipeline);
    renderPass.setBindings({ uniforms: this.uniformBuffer });
    renderPass.setVertexArray(this.vertexArray);
    renderPass.draw({ vertexCount: vertices.length/FLOATS_PER_VERTEX });
    if(materialFrame) {
      this.pbr?.draw(renderPass, false);
    }
    // Resolve depth-dependent effects on opaque color, then composite translucent geometry
    // against the original opaque depth. Particles retain crisp coverage and need no fabricated velocity.
    if(this.sceneEffects && this.postprocessor) {
      renderPass.end();
      const scene = this.postprocessor.targets[0];
      let effectsFrame: SceneEffectsFrame = {depthTexture: scene.depth, ...sceneFrame};
      if(this.backend === GPU_BACKEND.WEBGPU && attachmentFrame) {
        this.sceneAttachments ??= new SceneEffectAttachments(this.device);
        effectsFrame = this.sceneAttachments.prepare(getSceneEffectsRequirements(this.sceneEffects.configuration),
          scene.depth, effectsFrame, attachmentFrame);
      }
      processedScene = this.sceneEffects.render(scene.texture, effectsFrame);
      if(this.compositeTexture !== processedScene || this.compositeDepth !== scene.depth) {
        this.destroyCompositeFramebuffer();
        this.compositeFramebuffer = this.device.createFramebuffer({
          id: 'scene-effects-transparency-composite', width: scene.texture.width, height: scene.texture.height,
          colorAttachments: [processedScene], depthStencilAttachment: scene.depth
        });
        this.compositeTexture = processedScene;
        this.compositeDepth = scene.depth;
      }
      renderPass = this.device.beginRenderPass({framebuffer: this.compositeFramebuffer!, clearColor: false, clearDepth: false});
    }
    if(mixedTransparency && materialFrame && this.pbr) {
      const sorted = collectTransparentRanges(
        materialFrame.backward, vertices.length/FLOATS_PER_VERTEX,
        transparentVertices, transparentDraws, this.pbr.transparentRanges
      );
      for(const range of sorted) {
        if(range.kind === 'pbr') {
          this.pbr.drawRange(renderPass, range, true);
        } else if(range.kind === 'textured') {
          this.textured?.draw(renderPass, this.uniformBuffer, range.texture, range.firstVertex, range.vertexCount);
        } else {
          renderPass.setPipeline(this.transparentPipeline);
          renderPass.setBindings({uniforms: this.uniformBuffer});
          renderPass.setVertexArray(this.vertexArray);
          renderPass.draw({firstVertex: range.firstVertex, vertexCount: range.vertexCount});
        }
      }
    } else {
      if(transparentVertices.length > 0) {
        renderPass.setPipeline(this.transparentPipeline);
        renderPass.setBindings({ uniforms: this.uniformBuffer });
        renderPass.setVertexArray(this.vertexArray);
        renderPass.draw({
          firstVertex: vertices.length/FLOATS_PER_VERTEX,
          vertexCount: transparentVertices.length/FLOATS_PER_VERTEX
        });
      }
      let coloredVertex = (vertices.length + transparentVertices.length)/FLOATS_PER_VERTEX;
      let texturedVertex = 0;
      for(const draw of transparentDraws) {
        if(draw.texture) {
          const count = draw.vertices.length/9;
          this.textured?.draw(renderPass, this.uniformBuffer, draw.texture, texturedVertex, count);
          texturedVertex+= count;
        } else {
          const count = draw.vertices.length/FLOATS_PER_VERTEX;
          renderPass.setPipeline(this.transparentPipeline);
          renderPass.setBindings({ uniforms: this.uniformBuffer });
          renderPass.setVertexArray(this.vertexArray);
          renderPass.draw({ firstVertex: coloredVertex, vertexCount: count });
          coloredVertex+= count;
        }
      }
      if(materialFrame) {
        this.pbr?.draw(renderPass, true);
      }
    }
    renderPass.end();
    if(this.postprocessor && this.postProcessing) {
      this.postprocessor.render(this.postProcessing, timeSeconds, processedScene);
    }
    try {
      this.device.submit();
    } catch(error) {
      this.sceneEffects?.resetHistory();
      throw error;
    }
  }

  /** Release luma.gl resources when the owning game is disposed. */
  destroy() {
    if(this.destroyed) {
      return;
    }
    this.destroyed = true;
    this.releaseResources();
    this.canvasContext.destroy();
    this.device.destroy();
  }

  /**
   * Release dependent allocations before the canvas/device roots, including failed atmosphere setup.
   * @private
   */
  private destroyCompositeFramebuffer() {
    this.compositeFramebuffer?.destroy();
    this.compositeFramebuffer = null;
    this.compositeTexture = null;
    this.compositeDepth = null;
  }

  private releaseResources() {
    if(this.resourcesReleased) {
      return;
    }
    this.resourcesReleased = true;
    this.atmosphere?.destroy();
    this.pbr?.destroy();
    this.textured?.destroy();
    this.destroyCompositeFramebuffer();
    this.sceneAttachments?.destroy();
    this.sceneEffects?.destroy();
    this.postprocessor?.destroy();
    this.vertexArray.destroy();
    this.vertexBuffer.destroy();
    this.uniformBuffer.destroy();
    this.pipeline.destroy();
    this.transparentPipeline.destroy();
    if(this.opaqueShaders) {
      for(const shader of new Set(Object.values(this.opaqueShaders))) {
        shader.destroy();
      }
    }
    // WGSL uses one combined shader object while GLSL creates one per stage.
    for(const shader of new Set(Object.values(this.shaders))) {
      shader.destroy();
    }
  }
}

export interface ColoredRange {
  kind: 'colored';
  firstVertex: number;
  vertexCount: number;
  depth: number;
}
export interface TexturedRange {
  kind: 'textured';
  texture: Texture;
  firstVertex: number;
  vertexCount: number;
  depth: number;
}
export type MaterialRange = {
    kind: 'pbr';
} & PbrTransparentRange;
export type TransparentRange = ColoredRange | TexturedRange | MaterialRange;

/** Merge borrowed colored, textured and PBR streams in world-space camera order.
 * Existing callers without a material frame retain their supplied ordering.
 * @param backward Camera direction toward the viewer.
 * @param opaqueVertexCount Offset preceding the colored transparent stream.
 * @param vertices Colored transparent triangles.
 * @param draws
 * @param materialRanges
 */
function collectTransparentRanges(backward: readonly number[], opaqueVertexCount: number, vertices: number[] | Float32Array, draws: readonly TransparentDraw[], materialRanges: readonly PbrTransparentRange[]): TransparentRange[] {

  const ranges: TransparentRange[] = [];
  appendTransparentRanges(ranges, vertices, opaqueVertexCount, backward);
  let coloredOffset = opaqueVertexCount + vertices.length/FLOATS_PER_VERTEX;
  let texturedOffset = 0;
  for(const draw of draws) {
    if(draw.texture) {
      appendTransparentRanges(ranges, draw.vertices, texturedOffset, backward, draw.texture);
      texturedOffset+= draw.vertices.length/9;
    } else {
      appendTransparentRanges(ranges, draw.vertices, coloredOffset, backward);
      coloredOffset+= draw.vertices.length/FLOATS_PER_VERTEX;
    }
  }
  for(const range of materialRanges) {
    ranges.push({kind: 'pbr', ...range});
  }
  ranges.sort((a, b) => a.depth - b.depth);

  const merged: TransparentRange[] = [];
  for(const range of ranges) {
    const previous = merged.at(-1);
    if(previous && mergeTransparentRange(previous, range)) {
      continue;
    }
    merged.push(range);
  }
  return merged;
}

/** Append one command per triangle; sorting later can split a supplied material run.
 * @param ranges @param vertices
 * @param firstVertex @param backward
 * @param [texture]
 */
function appendTransparentRanges(ranges: TransparentRange[], vertices: number[] | Float32Array, firstVertex: number, backward: readonly number[], texture?: Texture) {
  const stride = texture ? 9 : FLOATS_PER_VERTEX;
  for(let offset = 0; offset < vertices.length; offset+= stride*3) {
    let depth = 0;
    for(let corner = 0; corner < 3; corner++) {
      const index = offset + corner*stride;
      depth+= vertices[index]*backward[0] + vertices[index + 1]*backward[1] + vertices[index + 2]*backward[2];
    }
    const range = {firstVertex: firstVertex + offset/stride, vertexCount: 3, depth: depth/3};
    ranges.push(texture ? {kind: 'textured', texture, ...range} : {kind: 'colored', ...range});
  }
}

/** Coalesce only adjacent, contiguous ranges with identical material bindings.
 * @param previous @param next
 */
function mergeTransparentRange(previous: TransparentRange, next: TransparentRange) {
  if(previous.kind === 'pbr' && next.kind === 'pbr') {
    if(previous.material === next.material && previous.firstIndex + previous.indexCount === next.firstIndex) {
      previous.indexCount+= next.indexCount;
      return true;
    }
    return false;
  }
  if(previous.kind === 'pbr' || next.kind === 'pbr' || previous.kind !== next.kind) {
    return false;
  }
  if(previous.kind === 'textured' && next.kind === 'textured' && previous.texture !== next.texture) {
    return false;
  }
  if(previous.firstVertex + previous.vertexCount !== next.firstVertex) {
    return false;
  }
  previous.vertexCount+= next.vertexCount;
  return true;
}
