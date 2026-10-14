// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {
  RenderPipelineProps,
  Framebuffer,
  Texture,
  Device,
  CanvasContext,
  Shader,
  RenderPipeline,
  RenderPass,
  VertexArray
} from '@luma.gl/core';
import type {
  PbrMesh,
  Color,
  PbrFrame
} from '../core/types.ts';
import type {
  FrameView,
  FrameViewOptions
} from './scene/frame-view.ts';
import type {
  PassCamera,
  RenderPassContext
} from './scene/render-pass-context.ts';
import type {
  SceneLightFrame,
  ShadowOptions
} from './shadows/types.ts';
import type {
  RendererDiagnosticOptions,
  RendererDiagnostics
} from './gpu/gpu-diagnostics.ts';
import type {
  SceneEffectsOptions,
  SceneEffectsFrame
} from './scene/scene-effects.ts';
import type { GpuDeviceOptions } from './gpu/gpu-device.ts';
import type { TransparentDraw } from './materials/textured-material-pass.ts';
import type { PbrTransparentRange } from './materials/pbr/pbr-material-pass.ts';
import type { PostProcessingConfig } from './postprocessor/config.ts';
import type {
  PbrEnvironmentPixels,
  PbrEnvironmentOptions
} from './environment/pbr-environment.ts';
import type { SceneAttachmentFrame } from './scene/scene-effect-attachments.ts';

import { Buffer } from '@luma.gl/core';
import {
  GPU_BACKEND,
  GPU_COMPARE,
  GPU_TEXTURE_FORMAT,
  GPU_TOPOLOGY,
  GPU_VERTEX_FORMAT,
  SHADER_ENTRY_POINT
} from './gpu/constants.ts';
import { SceneLitPass } from './shadows/scene-lit-pass.ts';
import {SceneShadowPass} from './shadows/scene-shadow-pass.ts';
import type {NativeSceneGeometry} from './scene/native-geometry.ts';
import { createRenderPassContext } from './scene/render-pass-context.ts';
import { createFrameView } from './scene/frame-view.ts';
import { readGpuDiagnostics } from './gpu/gpu-diagnostics.ts';
import { AtmospherePass } from './environment/atmosphere-pass.ts';
import { PbrEnvironment } from './environment/pbr-environment.ts';
import { PbrMaterialPass } from './materials/pbr/pbr-material-pass.ts';
import { TexturedMaterialPass } from './materials/textured-material-pass.ts';
import { Postprocessor } from './postprocessor/postprocessor.ts';
import { SceneEffectAttachments } from './scene/scene-effect-attachments.ts';
import { getSceneEffectsRequirements } from './scene/scene-effect-config.ts';
import { SceneEffects } from './scene/scene-effects.ts';
import { DynamicGpuBuffer } from './gpu/dynamic-gpu-buffer.ts';
import {
  createGpuCanvasDevice,
  createPortableShaders
} from './gpu/gpu-device.ts';
import { FLOATS_PER_VERTEX } from './geometry/geometry.ts';
import {
  addPostProcessingShaders,
  createPostProcessingUniforms,
  normalizePostProcessingConfig
} from './postprocessor/config.ts';

/**
 * Borrowed shader sources defining matching WebGPU and WebGL triangle programs.
 */
export interface TriangleShaderSources {
  wgsl: string;
  glsl: {
    vertex: string;
    fragment: string;
  };
}

/**
 * Construction settings; optional passes remain tied to this renderer's device.
 * Buffer sizes are bytes. Scene effects require explicitly enabled post-processing.
 * Shader strings and options are borrowed during construction; live settings are copied.
 */
export interface RendererOptions {
  shaderSources: TriangleShaderSources;
  texturedShaderSources?: TriangleShaderSources;
  opaqueShaderSources?: TriangleShaderSources;
  uniformBufferSize?: number;
  initialVertexBufferSize?: number;
  pbr?: boolean;
  shadows?: ShadowOptions | false;
  atmosphere?: boolean;
  postProcessing?: Partial<PostProcessingConfig>;
  sceneEffects?: SceneEffectsOptions;
  device?: GpuDeviceOptions;
}

/**
 * Colored vertex-buffer command with average world-space depth along camera backward.
 */
export interface ColoredRange {
  kind: 'colored';
  firstVertex: number;
  vertexCount: number;
  depth: number;
}

/**
 * Textured vertex-buffer command; texture ownership stays with its material source.
 */
export interface TexturedRange {
  kind: 'textured';
  texture: Texture;
  firstVertex: number;
  vertexCount: number;
  depth: number;
}

/**
 * PBR index-buffer command borrowing its material from the prepared material pass.
 */
export type MaterialRange = {
  kind: 'pbr';
} & PbrTransparentRange;

/**
 * Commands borrowing existing GPU streams for one transparent scene pass.
 */
export type TransparentRange = ColoredRange | TexturedRange | MaterialRange;

// Stable command identifiers preserve each source stream's material bindings.
const TRANSPARENT_RANGE_KIND = {
  COLORED: 'colored',
  TEXTURED: 'textured',
  PBR: 'pbr'
} as const;
const POST_PROCESSING_SHADER_MACRO = 'JOY_POST_APPLY_MACRO';

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
  // Owned device roots; all dependent passes and buffers are released first.
  declare readonly device: Device;
  declare readonly canvasContext: CanvasContext;
  declare readonly backend: typeof GPU_BACKEND.WEBGL | typeof GPU_BACKEND.WEBGPU | "null" | "unknown";

  // Fixed geometry resources; buffer storage can grow without replacing its owner.
  declare readonly sceneUniformBufferSize: number;
  declare readonly opaqueShaders: {
    vertex: Shader;
    fragment: Shader;
  } | null;
  declare readonly shaders: ReturnType<typeof createPortableShaders>;
  declare readonly pipeline: RenderPipeline;
  declare readonly transparentPipeline: RenderPipeline;
  declare readonly vertexArray: VertexArray;
  declare readonly uniformBuffer: Buffer;
  declare readonly vertexBuffer: DynamicGpuBuffer;

  // Optional owned passes borrow the device. Effects and attachments can be replaced.
  declare readonly postprocessor: Postprocessor | null;
  declare readonly pbr: PbrMaterialPass | null;
  declare readonly textured: TexturedMaterialPass | null;
  declare readonly atmosphere: AtmospherePass | null;
  declare sceneEffects: SceneEffects | null;
  private litShadowPass: SceneLitPass | null = null;
  private shadowPass: SceneShadowPass | null = null;
  private shadowAllocationKey = '';
  private shadowFrameActive = false;
  private sceneAttachments: SceneEffectAttachments | null = null;

  // Owned framebuffer borrows effect color and opaque depth; it never destroys them.
  private compositeFramebuffer: Framebuffer | null = null;
  private compositeTexture: Texture | null = null;
  private compositeDepth: Texture | null = null;

  // Authored configuration stays distinct from mutable surface and lifecycle state.
  declare postProcessing: PostProcessingConfig | null;
  private readonly shadowOptions: ShadowOptions | false;
  declare width: number;
  declare height: number;
  declare drawingBufferWidth: number;
  declare drawingBufferHeight: number;
  declare resourcesReleased: boolean;
  declare destroyed: boolean;

  /**
   * Create a renderer and transfer its device/canvas ownership to the result.
   * Failed construction destroys the newly created roots before rejecting.
   * @param canvas Borrowed DOM canvas dedicated to GPU rendering; never removed here.
   * @param options Shader sources, optional features, and device creation settings.
   * @returns An owned renderer; its caller must destroy it after dependent environments.
   */
  static async create(
    canvas: HTMLCanvasElement,
    options: RendererOptions): Promise<GpuTriangleRenderer> {
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
   * Initialize owned geometry and optional scene passes in dependency order.
   * Direct construction transfers device roots on success; callers handle failed roots.
   * Invalid scene-effect settings and GPU allocation failures throw synchronously.
   * @param device Device owned by this renderer after successful construction.
   * @param canvasContext Canvas context owned alongside the device.
   * @param options Borrowed shaders and feature settings; buffer capacities are bytes.
   */
  constructor(
    device: Device,
    canvasContext: CanvasContext,
    {
      shaderSources,
      texturedShaderSources,
      opaqueShaderSources,
      uniformBufferSize = 16,
      initialVertexBufferSize = 64*1024,
      postProcessing,
      sceneEffects,
      pbr = false,
      atmosphere = false,
      shadows = {}
    }: RendererOptions) {
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
    // PBR lighting batches accumulate linear radiance before a single display resolve.
    this.postProcessing = postProcessing || pbr ? normalizePostProcessingConfig(postProcessing ?? {}) : null;
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
    this.shadowOptions = shadows === false ? false : Object.freeze({ ...shadows });
    this.pbr = pbr ? new PbrMaterialPass(device, this.postprocessor?.format) : null;
    // Templates need a fragment output even when this renderer draws directly to
    // the canvas. Fully authored caller shaders require no template expansion.
    const needsShaderExpansion = postProcessing || shaderSources.wgsl?.includes(POST_PROCESSING_SHADER_MACRO);
    const resolvedShaderSources = needsShaderExpansion
      ? addPostProcessingShaders(shaderSources, {
        includeUniforms: Boolean(this.postProcessing)
      })
      : shaderSources;
    this.textured = texturedShaderSources ? new TexturedMaterialPass(
      device,
      texturedShaderSources,
      this.postprocessor?.format
    ) : null;

    this.opaqueShaders = opaqueShaderSources
      ? createPortableShaders(device, opaqueShaderSources, 'lit-opaque-triangles')
      : null;
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
          {
            attribute: 'position',
            format: GPU_VERTEX_FORMAT.FLOAT32X3,
            byteOffset: 0
          },
          {
            attribute: 'color',
            format: GPU_VERTEX_FORMAT.FLOAT32X4,
            byteOffset: 12
          }
        ]
      }],
      // Explicit format is required even when a pipeline only reads depth.
      depthStencilAttachmentFormat: GPU_TEXTURE_FORMAT.DEPTH24PLUS,
      ...(this.postprocessor ? { colorAttachmentFormats: [this.postprocessor.format] } : {})
    };
    this.pipeline = device.createRenderPipeline({
      id: 'triangle-opaque-pipeline',
      ...pipelineOptions,
      ...(this.opaqueShaders ? {
        vs: this.opaqueShaders.vertex,
        fs: this.opaqueShaders.fragment
      } : {}),
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
    const postProcessingUniformBytes = this.postProcessing
      ? createPostProcessingUniforms(this.postProcessing, 0, 0, 0).byteLength
      : 0;
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
   * not this renderer alone. Performs no GPU readback.
   * @param options Counters to sample; disabled groups are omitted from the snapshot.
   * @returns A new diagnostic snapshot, or null after this renderer is destroyed.
   */
  getDiagnostics(options: RendererDiagnosticOptions = {}): RendererDiagnostics | null {
    if(this.destroyed) {
      return null;
    }
    const snapshot = readGpuDiagnostics(this.device, options);
    const lit = this.shadowFrameActive ? this.shadowPass?.shadows.stats : null;
    const pbr = this.pbr?.shadowStats;
    snapshot.shadows = {
      available: this.backend === GPU_BACKEND.WEBGPU && this.shadowOptions !== false,
      castingPointLights: (lit?.castingPointLights ?? 0) + (pbr?.castingPointLights ?? 0),
      castingDirectionalLights: (lit?.castingDirectionalLights ?? 0) + (pbr?.castingDirectionalLights ?? 0),
      batches: (lit?.batches ?? 0) + (pbr?.batches ?? 0),
      views: (lit?.views ?? 0) + (pbr?.views ?? 0)
    };
    return snapshot;
  }

  /**
   * Build the shared immutable view using this renderer's fixed shadow configuration.
   * Call once per submission before preparing features; input camera and light records are copied.
   * Shadow quality, map sizes, and biases remain fixed for this renderer's lifetime.
   * @param options Borrowed camera, light, environment, and frame-timing inputs.
   * @returns A new immutable CPU snapshot; environment GPU bindings remain borrowed.
   */
  createFrameView(options: Omit<FrameViewOptions, 'shadows'>): FrameView {
    return createFrameView({
      ...options,
      shadows: this.shadowOptions
    });
  }

  /**
   * Upload a world-space material mesh; await readiness before displaying new document data.
   * Requires PBR at creation. Failed upload preserves the previous accepted mesh.
   * @param mesh Borrowed geometry and material data copied into renderer-owned resources.
   * @returns Whether the mesh was installed; false if superseded or already disposed.
   */
  async setMaterialMesh(mesh: PbrMesh): Promise<boolean> {
    if(!this.pbr) {
      throw new Error('Material meshes require pbr at renderer creation.');
    }
    return this.pbr.setMesh(mesh);
  }

  /**
   * Prepare original equirectangular pixels through luma. The caller owns the result
   * and must destroy it before this renderer/device; ordinary frames borrow it.
   * Invalid pixels, unsupported preparation, or a destroyed renderer throw.
   * @param pixels Borrowed RGBA equirectangular pixels; float RGB represents linear HDR.
   * @param options Borrowed environment integration settings.
   * @returns An owned prepared environment; failed preparation releases its allocations.
   */
  createMaterialEnvironment(
    pixels: PbrEnvironmentPixels,
    options: PbrEnvironmentOptions = {}): PbrEnvironment {
    if(this.destroyed) {
      throw new Error('Cannot prepare an environment on a destroyed renderer.');
    }
    const environment = new PbrEnvironment(this.device);
    try {
      environment.preparePixels(pixels, options);
      return environment;
    } catch(error) {
      environment.destroy();
      throw error;
    }
  }

  /**
   * Replace authored settings for the next frame without reallocating GPU resources.
   * Requires postProcessing at creation; omitted fields reset to engine defaults.
   * Invalid settings throw before modifying the last working configuration.
   * @param config Borrowed settings, copied on success.
   */
  setPostProcessingConfig(config: Partial<PostProcessingConfig>): void {
    if(!this.postprocessor) {
      throw new Error('Live post-processing requires postProcessing at renderer creation.');
    }
    this.postProcessing = normalizePostProcessingConfig(config);
  }

  /**
   * Replace the optional scene graph atomically; failed construction retains the previous graph.
   * Requires a live offscreen scene. Successful replacement releases attachments and history.
   * @param config Borrowed effect settings; the graph uses the renderer's color format.
   */
  setSceneEffectsConfig(config: SceneEffectsOptions): void {
    if(this.destroyed || !this.postprocessor) {
      throw new Error('Scene effects require a live offscreen scene renderer.');
    }
    const replacement = new SceneEffects(this.device, {
      ...config,
      colorFormat: this.postprocessor.format
    });
    this.destroyCompositeFramebuffer();
    this.sceneEffects?.destroy();
    this.sceneEffects = replacement;
    this.sceneAttachments?.destroy();
    this.sceneAttachments = null;
    this.destroyCompositeFramebuffer();
  }

  /**
   * Invalidate owned temporal history after a camera cut or scene replacement.
   * Has no effect when scene effects are disabled.
   */
  resetSceneHistory(): void {
    this.sceneEffects?.resetHistory();
  }

  /**
   * Read the current auxiliary volume-light statistics without allocating GPU resources.
   * @returns Borrowed statistics owned by the attachment pass, or null before it exists.
   */
  get sceneVolumeStats() {
    return this.sceneAttachments?.volumeStats ?? null;
  }

  /**
   * Bind the interleaved stream using each backend's vertex-array semantics.
   * The allocation remains owned by its source; this method only replaces vertex bindings.
   * @param buffer Borrowed buffer, defaulting to the renderer's current geometry allocation.
   */
  bindVertexBuffer(buffer: Buffer = this.vertexBuffer?.buffer): void {
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
  resize(
    width: number,
    height: number,
    pixelRatio: number): void {
    this.width = width;
    this.height = height;
    this.drawingBufferWidth = Math.max(1, Math.round(width*pixelRatio));
    this.drawingBufferHeight = Math.max(1, Math.round(height*pixelRatio));
    this.canvasContext.setDrawingBufferSize(this.drawingBufferWidth, this.drawingBufferHeight);
    this.postprocessor?.resize(this.drawingBufferWidth, this.drawingBufferHeight, width, height);
  }

  /**
   * Submit opaque geometry, then sorted translucent geometry; clear even if empty.
   * CPU arrays and records remain unchanged. Borrowed textures must outlive submission.
   * Missing optional pass support, invalid uniform size, and GPU failures throw.
   * Submission failure invalidates temporal history before rethrowing.
   * @param vertices Borrowed world-space triangle list of x, y, z, r, g, b, a values.
   * @param uniformData Borrowed scene values matching the configured uniform byte size.
   * @param clearColor Borrowed RGBA frame background.
   * @param transparentVertices Borrowed back-to-front world-space XYZ/RGBA triangles.
   * @param timeSeconds Animation time in seconds used by temporal post effects.
   * @param transparentDraws Additional globally sorted XYZ/RGBA or XYZ/RGBA/UV material runs; textures require texturedShaderSources.
   * @param materialFrame Camera/lighting for the optional PBR mesh. Null hides it.
   * @param sceneFrame Borrowed effect inputs; depth defaults to the owned scene attachment.
   * Supply real velocities and projection jitter; reset history after cuts or scene replacement.
   * @param [atmosphereUniforms] Optional packed background snapshot. Requires atmosphere at creation; null hides it.
   * @param [attachmentFrame] Borrowed geometry and shared FrameView for auxiliary scene attachments.
   * @param lightFrame Shared immutable view and complete shadow casters. Pass cameras/targets remain local.
   * Standalone views must retain this renderer's construction-time shadow configuration.
   */
  render(
    vertices: number[] | Float32Array,
    uniformData: Float32Array<ArrayBuffer>,
    clearColor: Color,
    transparentVertices: number[] | Float32Array = [],
    timeSeconds: number = 0,
    transparentDraws: readonly TransparentDraw[] = [],
    materialFrame: PbrFrame | null = null,
    sceneFrame: SceneEffectsFrame = {},
    atmosphereUniforms: Float32Array<ArrayBuffer> | null = null,
    attachmentFrame: SceneAttachmentFrame | null = null,
    lightFrame: SceneLightFrame | null = null,
    nativeGeometry: readonly NativeSceneGeometry[] = []): void {
    if(atmosphereUniforms && !this.atmosphere) {
      throw new Error('Atmosphere uniforms require atmosphere at renderer creation.');
    }

    this.prepareVertexStreams(vertices, transparentVertices, transparentDraws);
    this.prepareFrameUniforms(uniformData, timeSeconds);

    // Merge material streams only when PBR and caller geometry share the transparent phase.
    const mixedTransparency = Boolean(materialFrame && (transparentVertices.length || transparentDraws.length));
    const shadows = this.prepareSceneLighting(materialFrame, mixedTransparency, lightFrame, uniformData, nativeGeometry);
    let processedScene: Texture | undefined;

    // Pass targets and raster camera overrides remain separate from the immutable main view.
    const camera = lightFrame?.view.camera;
    const passCamera: PassCamera | null = camera ? Object.freeze({
      viewMatrix: camera.viewMatrix,
      projectionMatrix: camera.rasterProjectionMatrix,
      viewProjectionMatrix: camera.rasterViewProjectionMatrix,
      nearPlane: camera.nearPlane,
      farPlane: camera.farPlane
    }) : null;
    const target = this.postprocessor?.sceneFramebuffer ?? this.canvasContext.getCurrentFramebuffer({
      depthStencilFormat: GPU_TEXTURE_FORMAT.DEPTH24PLUS
    });
    let context = createRenderPassContext(this.device.beginRenderPass({
      framebuffer: target,
      clearColor: [clearColor[0], clearColor[1], clearColor[2], clearColor[3]],
      clearDepth: 1
    }), target, passCamera);

    this.drawOpaqueScene(
      context.renderPass,
      vertices.length/FLOATS_PER_VERTEX,
      Boolean(lightFrame && shadows !== false),
      materialFrame,
      atmosphereUniforms,
      lightFrame,
      nativeGeometry
    );

    // Resolve depth-dependent effects on opaque color, then reuse its depth for transparency.
    // Particles retain crisp coverage and need no fabricated velocity.
    if(this.sceneEffects && this.postprocessor) {
      processedScene = this.resolveSceneEffects(
        context,
        sceneFrame,
        attachmentFrame,
        this.sceneEffects,
        this.postprocessor
      );
      context = createRenderPassContext(this.device.beginRenderPass({
        framebuffer: this.compositeFramebuffer!,
        clearColor: false,
        clearDepth: false
      }), this.compositeFramebuffer!, passCamera);
    }

    this.drawTransparentScene(
      context.renderPass,
      vertices,
      transparentVertices,
      transparentDraws,
      materialFrame,
      mixedTransparency
    );
    context.renderPass.end();

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

  /**
   * Release owned passes, buffers, shaders, canvas context, and device in dependency order.
   * Repeated destruction is harmless. Caller-owned environments must be destroyed first.
   */
  destroy(): void {
    if(this.destroyed) {
      return;
    }
    this.destroyed = true;
    this.releaseResources();
    this.canvasContext.destroy();
    this.device.destroy();
  }

  /**
   * Validate texture support, pack colored streams, and upload each geometry source once.
   * Textured preparation precedes colored upload, preserving allocation/failure order.
   * @param vertices Borrowed opaque world-space XYZ/RGBA triangle stream.
   * @param transparentVertices Borrowed sorted world-space XYZ/RGBA triangle stream.
   * @param transparentDraws Borrowed sorted colored or textured material runs.
   */
  private prepareVertexStreams(
    vertices: number[] | Float32Array,
    transparentVertices: number[] | Float32Array,
    transparentDraws: readonly TransparentDraw[]): void {
    // Upload once; draw ranges select the two materials without reallocating
    // buffers or resetting depth between the opaque and translucent stages.
    if(!this.textured && transparentDraws.some(draw => draw.texture)) {
      throw new Error('Textured draws require texturedShaderSources at renderer creation.');
    }
    const additionalLength = transparentDraws.reduce(
      (total, draw) => total + (draw.texture ? 0 : draw.vertices.length),
      0
    );
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
  }

  /**
   * Validate the scene prefix and upload a copy with the optional post-processing tail.
   * The caller's uniform array remains unchanged; invalid byte length throws before upload.
   * @param uniformData Borrowed scene uniforms matching the construction-time byte size.
   * @param timeSeconds Animation time in seconds for temporal post-processing uniforms.
   */
  private prepareFrameUniforms(
    uniformData: Float32Array<ArrayBuffer>,
    timeSeconds: number): void {
    const sceneUniformBufferSize = this.sceneUniformBufferSize ?? uniformData.byteLength;
    if(uniformData.byteLength !== sceneUniformBufferSize) {
      throw new RangeError(`Expected ${sceneUniformBufferSize} bytes of scene uniforms, received ${uniformData.byteLength}.`);
    }
    if(this.postProcessing) {
      const postProcessingUniforms = createPostProcessingUniforms(
        this.postProcessing,
        this.drawingBufferWidth,
        this.drawingBufferHeight,
        timeSeconds
      );
      const combinedUniforms = new Float32Array(uniformData.length + postProcessingUniforms.length);
      combinedUniforms.set(uniformData);
      combinedUniforms.set(postProcessingUniforms, uniformData.length);
      this.uniformBuffer.write(combinedUniforms);
    } else {
      this.uniformBuffer.write(uniformData);
    }
  }

  /**
   * Prepare PBR geometry before shadow views so material casters share recorded matrices.
   * Missing PBR support throws before shadow work. Shadow resources are created lazily.
   * @param materialFrame Borrowed camera/lighting for PBR; null leaves the mesh hidden.
   * @param mixedTransparency Whether to prepare per-triangle PBR sorting ranges.
   * @param lightFrame Borrowed immutable scene view and complete world-space casters.
   * @param uniformData Borrowed packed scene uniforms for colored shadow receivers.
   * @returns This frame's shadow settings, or undefined when no view supplies them.
   */
  private prepareSceneLighting(
    materialFrame: PbrFrame | null,
    mixedTransparency: boolean,
    lightFrame: SceneLightFrame | null,
    uniformData: Float32Array<ArrayBuffer>,
    nativeGeometry: readonly NativeSceneGeometry[]): ShadowOptions | false | undefined {
    if(materialFrame) {
      if(!this.pbr) {
        throw new Error('Material frames require pbr at renderer creation.');
      }
      const preparedMaterialFrame: PbrFrame = {
        ...materialFrame,
        shadows: this.shadowOptions === false ? false : (materialFrame.shadows ?? this.shadowOptions)
      };
      this.pbr.prepare(preparedMaterialFrame, mixedTransparency, lightFrame?.casters);
    }
    const shadows = this.shadowOptions === false ? false : lightFrame?.view.shadows;
    this.shadowFrameActive = Boolean(lightFrame && this.backend === GPU_BACKEND.WEBGPU && shadows !== false);
    if(lightFrame && this.backend === GPU_BACKEND.WEBGPU && shadows) {
      const {normalBias, shadowDistance, ...allocationOptions} = shadows;
      const allocationKey = JSON.stringify(allocationOptions);
      if(!this.shadowPass || this.shadowAllocationKey !== allocationKey) {
        const replacement = new SceneShadowPass(this.device, shadows);
        this.shadowPass?.destroy();
        this.shadowPass = replacement;
        this.shadowAllocationKey = allocationKey;
      }
      this.shadowPass.shadows.setReceiverOptions({normalBias, shadowDistance});
      const batches = this.shadowPass.prepare(lightFrame, (pass, matrix) => {
        if(materialFrame) {
          this.pbr?.drawShadow(pass, matrix);
        }
        for(const geometry of nativeGeometry) {
          if(geometry.castsShadow) {
            geometry.drawShadow(pass, matrix);
          }
        }
      });
      this.litShadowPass ??= new SceneLitPass(this.device, this.postprocessor?.format);
      this.litShadowPass.prepare(lightFrame.view.camera, uniformData, batches);
    }
    return shadows;
  }

  /**
   * Record background, colored opaque geometry, and PBR opaque geometry in that order.
   * Borrows the open pass and leaves it open for scene effects or transparency.
   * @param renderPass Caller-owned pass targeting the scene's color and depth.
   * @param opaqueVertexCount Number of colored vertices, including zero for an empty scene.
   * @param useShadowPass Whether this frame requests the prepared colored lighting pass.
   * @param materialFrame Borrowed PBR frame; null hides the material mesh.
   * @param atmosphereUniforms Borrowed packed background uniforms; null hides the sky.
   */
  private drawOpaqueScene(
    renderPass: RenderPass,
    opaqueVertexCount: number,
    useShadowPass: boolean,
    materialFrame: PbrFrame | null,
    atmosphereUniforms: Float32Array<ArrayBuffer> | null,
    lightFrame: SceneLightFrame | null,
    nativeGeometry: readonly NativeSceneGeometry[]): void {
    if(atmosphereUniforms) {
      this.atmosphere?.draw(renderPass, atmosphereUniforms);
    }
    if(useShadowPass && this.litShadowPass) {
      this.litShadowPass.draw(renderPass, this.vertexArray, opaqueVertexCount);
    } else {
      renderPass.setPipeline(this.pipeline);
      renderPass.setBindings({ uniforms: this.uniformBuffer });
      renderPass.setVertexArray(this.vertexArray);
      renderPass.draw({ vertexCount: opaqueVertexCount });
    }
    if(materialFrame) {
      this.pbr?.draw(renderPass, false);
    }
    for(const geometry of nativeGeometry) {
      if(!geometry.visible) {
        continue;
      }
      if(lightFrame && this.shadowFrameActive && this.litShadowPass && geometry.receivesShadow) {
        this.litShadowPass.drawNative(renderPass, (pass, bindings, additive) => {
          geometry.draw(pass, bindings, true, additive);
        });
      } else {
        geometry.draw(renderPass, {uniforms: this.uniformBuffer}, false, false);
      }
    }
  }

  /**
   * End opaque recording, prepare auxiliary attachments, and resolve effects before transparency.
   * Requires live effects and post-processing passes, checked by the frame control path.
   * Reuse the composite framebuffer only while both borrowed attachments remain identical.
   * @param context Borrowed opaque pass context; this method ends its render pass.
   * @param sceneFrame Borrowed effect inputs copied before auxiliary attachments are added.
   * @param attachmentFrame Borrowed geometry/view inputs, or null to use supplied textures.
   * @param sceneEffects Borrowed live effect graph from the frame control path.
   * @param postprocessor Borrowed live offscreen target owner from the frame control path.
   * @returns Effect-owned processed color, valid until the next render/resize or destruction.
   */
  private resolveSceneEffects(
    context: RenderPassContext,
    sceneFrame: SceneEffectsFrame,
    attachmentFrame: SceneAttachmentFrame | null,
    sceneEffects: SceneEffects,
    postprocessor: Postprocessor): Texture {
    context.renderPass.end();
    const scene = postprocessor.targets[0];
    let effectsFrame: SceneEffectsFrame = {
      depthTexture: scene.depth,
      ...sceneFrame
    };
    if(this.backend === GPU_BACKEND.WEBGPU && attachmentFrame) {
      this.sceneAttachments ??= new SceneEffectAttachments(this.device);
      effectsFrame = this.sceneAttachments.prepare(
        getSceneEffectsRequirements(sceneEffects.configuration),
        scene.depth,
        effectsFrame,
        attachmentFrame
      );
    }
    const processedScene = sceneEffects.render(scene.texture, effectsFrame);
    if(this.compositeTexture !== processedScene || this.compositeDepth !== scene.depth) {
      this.destroyCompositeFramebuffer();
      this.compositeFramebuffer = this.device.createFramebuffer({
        id: 'scene-effects-transparency-composite',
        width: scene.texture.width,
        height: scene.texture.height,
        colorAttachments: [processedScene],
        depthStencilAttachment: scene.depth
      });
      this.compositeTexture = processedScene;
      this.compositeDepth = scene.depth;
    }
    return processedScene;
  }

  /**
   * Record transparency against opaque depth without clearing or ending the borrowed pass.
   * Mixed PBR runs sort globally; other callers retain their supplied ordering.
   * @param renderPass Caller-owned open pass targeting opaque or processed scene color.
   * @param vertices Borrowed opaque stream, used only for colored vertex offsets.
   * @param transparentVertices Borrowed back-to-front colored world-space triangles.
   * @param transparentDraws Borrowed additional sorted colored/textured material runs.
   * @param materialFrame Borrowed PBR frame, or null to hide material transparency.
   * @param mixedTransparency Whether caller and PBR streams share a global ordering.
   */
  private drawTransparentScene(
    renderPass: RenderPass,
    vertices: number[] | Float32Array,
    transparentVertices: number[] | Float32Array,
    transparentDraws: readonly TransparentDraw[],
    materialFrame: PbrFrame | null,
    mixedTransparency: boolean): void {
    if(mixedTransparency && materialFrame && this.pbr) {
      const sorted = collectTransparentRanges(
        materialFrame.backward,
        vertices.length/FLOATS_PER_VERTEX,
        transparentVertices,
        transparentDraws,
        this.pbr.transparentRanges
      );
      for(const range of sorted) {
        if(range.kind === TRANSPARENT_RANGE_KIND.PBR) {
          this.pbr.drawRange(renderPass, range, true);
        } else if(range.kind === TRANSPARENT_RANGE_KIND.TEXTURED) {
          this.textured?.draw(
            renderPass,
            this.uniformBuffer,
            range.texture,
            range.firstVertex,
            range.vertexCount
          );
        } else {
          renderPass.setPipeline(this.transparentPipeline);
          renderPass.setBindings({ uniforms: this.uniformBuffer });
          renderPass.setVertexArray(this.vertexArray);
          renderPass.draw({
            firstVertex: range.firstVertex,
            vertexCount: range.vertexCount
          });
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
  }

  /**
   * Release the owned composite framebuffer and forget its borrowed attachment identities.
   * Effect color and opaque depth remain owned by their original passes.
   */
  private destroyCompositeFramebuffer(): void {
    this.compositeFramebuffer?.destroy();
    this.compositeFramebuffer = null;
    this.compositeTexture = null;
    this.compositeDepth = null;
  }

  /**
   * Release dependent allocations before device roots, including failed atmosphere setup.
   * Repeated calls do nothing; shared shader objects are destroyed only once per program.
   */
  private releaseResources(): void {
    if(this.resourcesReleased) {
      return;
    }
    this.resourcesReleased = true;
    this.litShadowPass?.destroy();
    this.shadowPass?.destroy();
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

/**
 * Merge borrowed colored, textured, and PBR streams in world-space camera order.
 * Input records remain unchanged; only newly allocated commands are sorted and coalesced.
 * @param backward World-space camera direction toward the viewer.
 * @param opaqueVertexCount Vertex offset preceding the colored transparent stream.
 * @param vertices Borrowed world-space XYZ/RGBA transparent triangles.
 * @param draws Borrowed additional colored/textured triangle runs in supplied order.
 * @param materialRanges Borrowed PBR commands whose depths use the same camera direction.
 * @returns New back-to-front commands borrowing source GPU/material bindings.
 */
function collectTransparentRanges(
  backward: readonly number[],
  opaqueVertexCount: number,
  vertices: number[] | Float32Array,
  draws: readonly TransparentDraw[],
  materialRanges: readonly PbrTransparentRange[]): TransparentRange[] {
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
    ranges.push({
      kind: TRANSPARENT_RANGE_KIND.PBR,
      ...range
    });
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

/**
 * Append one new command per triangle so sorting can split a supplied material run.
 * @param ranges Caller-owned output list mutated by appending commands.
 * @param vertices Borrowed world-space XYZ/RGBA, or XYZ/RGBA/UV when textured.
 * @param firstVertex Source buffer vertex offset preceding this run.
 * @param backward World-space camera direction toward the viewer for average depth.
 * @param texture Borrowed texture binding; omitted for colored commands.
 */
function appendTransparentRanges(
  ranges: TransparentRange[],
  vertices: number[] | Float32Array,
  firstVertex: number,
  backward: readonly number[],
  texture?: Texture): void {
  const stride = texture ? 9 : FLOATS_PER_VERTEX;
  for(let offset = 0; offset < vertices.length; offset+= stride*3) {
    let depth = 0;
    for(let corner = 0; corner < 3; corner++) {
      const index = offset + corner*stride;
      depth+= (vertices[index]*backward[0]) + (vertices[index + 1]*backward[1]) + (vertices[index + 2]*backward[2]);
    }
    const range = {
      firstVertex: firstVertex + offset/stride,
      vertexCount: 3,
      depth: depth/3
    };
    ranges.push(texture ? {
      kind: TRANSPARENT_RANGE_KIND.TEXTURED,
      texture,
      ...range
    } : {
      kind: TRANSPARENT_RANGE_KIND.COLORED,
      ...range
    });
  }
}

/**
 * Coalesce only adjacent, contiguous ranges with identical material bindings.
 * @param previous Caller-owned preceding command; its count is extended on success.
 * @param next Borrowed following command, unchanged by this operation.
 * @returns Whether the previous command now includes the following range.
 */
function mergeTransparentRange(
  previous: TransparentRange,
  next: TransparentRange): boolean {
  if(previous.kind === TRANSPARENT_RANGE_KIND.PBR && next.kind === TRANSPARENT_RANGE_KIND.PBR) {
    if(previous.material === next.material && (previous.firstIndex + previous.indexCount) === next.firstIndex) {
      previous.indexCount+= next.indexCount;
      return true;
    }
    return false;
  }
  if(previous.kind === TRANSPARENT_RANGE_KIND.PBR || next.kind === TRANSPARENT_RANGE_KIND.PBR || previous.kind !== next.kind) {
    return false;
  }
  if(previous.kind === TRANSPARENT_RANGE_KIND.TEXTURED && next.kind === TRANSPARENT_RANGE_KIND.TEXTURED && previous.texture !== next.texture) {
    return false;
  }
  if((previous.firstVertex + previous.vertexCount) !== next.firstVertex) {
    return false;
  }
  previous.vertexCount+= next.vertexCount;
  return true;
}
