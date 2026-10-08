// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {
  GPU_BACKEND,
  GPU_COMPARE,
  GPU_FILTER,
  GPU_TEXTURE_FORMAT,
  GPU_TOPOLOGY,
  GPU_VERTEX_FORMAT,
  SHADER_ENTRY_POINT
} from '../gpu/constants.ts';
import type { TextureFormatColor, BindingDeclaration, RenderPass, Device, ComputePipeline, Shader, RenderPipeline, VertexArray, Texture } from '@luma.gl/core';
import type { TerrainView, TerrainSelection } from './terrain-selection.ts';
import type { TerrainAsset } from './terrain-asset.ts';
import { Buffer } from '@luma.gl/core';
import { createPortableShaders } from '../gpu/gpu-device.ts';
import { createTerrainGrid } from './terrain-grid.ts';
import { selectTerrain, validateTerrainView } from './terrain-selection.ts';
import { TerrainTiming } from './terrain-timing.ts';
import wgsl from '../../../shaders/rendering/terrain/terrain.wgsl?raw';
import vertex from '../../../shaders/rendering/terrain/terrain.vert.glsl?raw';
import fragment from '../../../shaders/rendering/terrain/terrain.frag.glsl?raw';
import computeSource from '../../../shaders/rendering/terrain/select.wgsl?raw';

/**
 * Resident indexed terrain pass. Borrows device, asset and caller's depth-tested
 * render pass. Call prepare before starting the render pass; WebGPU writes and
 * ordered compute must precede draws. Destroy before the borrowed device.
 */
export class TerrainRenderer {
  declare device: Device;
  declare asset: TerrainAsset;
  declare disposed: boolean;
  declare resources: { destroy(): void; }[];
  declare grid: ReturnType<typeof createTerrainGrid>;
  declare frameData: Float32Array<ArrayBuffer>;
  declare patchData: Float32Array<ArrayBuffer>;
  declare selection: TerrainSelection[];
  declare patchUniforms: Buffer[];
  declare compute: ComputePipeline[];
  declare shaders: Shader[];
  declare stats: { patches: number; triangles: number; maxTriangles: number; cpuMilliseconds: number; gpuMilliseconds: number | null; gpuTiming: string; residentCpuBytes: number; residentGpuBytes: number; frameUploadBytes: number; triangleCountKind: string; };
  declare timing: TerrainTiming;
  declare pipeline: RenderPipeline;
  declare vertexArray: VertexArray;
  declare vertices: Buffer;
  declare indices: Buffer;
  declare scene: Buffer;
  declare heightTexture: Texture;
  declare descriptors: Buffer | null;
  declare indirect: Buffer | null;

  /**
   * @param device
   * @param asset
   * @param [colorFormat]
   */
  constructor(device: Device, asset: TerrainAsset, colorFormat?: TextureFormatColor) {
    // Borrowed dependencies outlive this renderer; only resources registered below are released here. Registration also makes partial startup safe to unwind.
    this.device = device;
    this.asset = asset;
    this.disposed = false;

    this.resources = [];

    // All patches reuse one sample-space grid. Heights stay in a resident texture;  moving the camera changes descriptors and index ranges, never vertex buffers.
    this.grid = createTerrainGrid(asset.manifest.tileCells);

    // Scratch arrays mirror Scene (matrix + four vec4s) and Patch (two vec4s). They are reused for uploads; patchUniforms hold persistent per-draw buffers.
    this.frameData = new Float32Array(32);
    this.patchData = new Float32Array(8);

    this.selection = [];

    this.patchUniforms = [];

    this.compute = [];

    this.shaders = [];

    // GPU selection stays on the GPU, so its triangle count is an upper bound.
    // Memory totals count numeric payloads, not driver or JavaScript object overhead.
    this.stats = {patches:asset.patches.length,triangles:0,maxTriangles:asset.patches.length*asset.manifest.tileCells**2*2,
      cpuMilliseconds:0,gpuMilliseconds:((null) as number | null),gpuTiming:'unsupported',
      residentCpuBytes:asset.heights.byteLength + this.grid.positions.byteLength + this.grid.indices.byteLength,
      residentGpuBytes:0,frameUploadBytes:128,triangleCountKind:device.type === GPU_BACKEND.WEBGPU ? 'upper bound (no readback)' : 'selected'};

    try {
      this.timing = this.own(new TerrainTiming(device));

      // Build the portable draw pipeline once. WGSL can share a shader object
      // across stages, so register each distinct object only once for cleanup.
      const shaders = createPortableShaders(device,{wgsl,glsl:{vertex,fragment}},'terrain');
      this.resources.push(...new Set(Object.values(shaders)));
      this.shaders.push(...new Set(Object.values(shaders)));
      this.pipeline = this.own(device.createRenderPipeline({
        id:'terrain-indexed',vs:shaders.vertex,fs:shaders.fragment,
        vertexEntryPoint:SHADER_ENTRY_POINT.VERTEX,fragmentEntryPoint:SHADER_ENTRY_POINT.FRAGMENT,topology:GPU_TOPOLOGY.TRIANGLE_LIST,
        // Explicit WGSL bindings match terrain.wgsl; WebGL reflects its uniform
        // blocks instead. Heights are fetched by texel, without float filtering.
        ...(device.type === GPU_BACKEND.WEBGPU ? {shaderLayout:{attributes:[{name:'gridPosition',location:0,type:'vec2<f32>'}],bindings:[
          {name:'scene',type:'uniform',group:0,location:0,visibility:3},
          {name:'heightTexture',type:'texture',group:0,location:1,sampleType:'unfilterable-float',visibility:1},
          {name:'patches',type:'read-only-storage',group:0,location:2,visibility:1},
          {name:'drawIndex',type:'uniform',group:0,location:3,visibility:1}
        ]}} : {}),
        bufferLayout:[{name:'grid',byteStride:8,attributes:[{attribute:'gridPosition',format:GPU_VERTEX_FORMAT.FLOAT32X2,byteOffset:0}]}],
        depthStencilAttachmentFormat:GPU_TEXTURE_FORMAT.DEPTH24PLUS,
        ...(colorFormat ? {colorAttachmentFormats:[colorFormat]} : {}),
        parameters:{depthWriteEnabled:true,depthCompare:GPU_COMPARE.LESS_EQUAL,cullMode:'none'}
      }));

      // Static topology and world-height samples are uploaded once at startup.
      this.vertexArray = this.own(device.createVertexArray({shaderLayout:this.pipeline.shaderLayout,bufferLayout:this.pipeline.bufferLayout}));
      this.vertices = this.buffer(this.grid.positions.byteLength,Buffer.VERTEX,this.grid.positions);
      this.indices = this.buffer(this.grid.indices.byteLength,Buffer.INDEX,this.grid.indices);
      this.vertexArray.setBuffer(0,this.vertices);
      this.vertexArray.setIndexBuffer(this.indices);
      this.scene = this.buffer(128,Buffer.UNIFORM);

      // Decode the global uint16 scale here so both vertex shaders read heights
      // directly in world units. Per-patch rescaling would introduce seams.
      const heights = Float32Array.from(asset.heights,value => asset.manifest.heightOffset + value/65535*asset.manifest.heightScale);
      this.heightTexture = this.own(device.createTexture({id:'terrain-height',width:asset.manifest.size,height:asset.manifest.size,format:GPU_TEXTURE_FORMAT.R32FLOAT,mipLevels:1,sampler:{minFilter:GPU_FILTER.NEAREST,magFilter:GPU_FILTER.NEAREST}}));
      this.heightTexture.copyImageData({data:heights});
      this.stats.residentGpuBytes+= heights.byteLength;

      // WebGPU writes one 32-byte Patch and one 20-byte indexed draw command
      // per leaf. WebGL instead receives the same Patch fields as uniforms.
      this.descriptors = device.type === GPU_BACKEND.WEBGPU ? this.buffer(asset.patches.length*32,Buffer.STORAGE | Buffer.COPY_SRC) : null;
      this.indirect = device.type === GPU_BACKEND.WEBGPU ? this.buffer(asset.patches.length*20,Buffer.STORAGE | Buffer.INDIRECT | Buffer.COPY_SRC) : null;
      if(device.type === GPU_BACKEND.WEBGPU) {
        // Each draw has a fixed patch ID; selection can change without uploading
        // IDs or requiring the optional indirect-first-instance feature.
        for(let i = 0; i < asset.patches.length; i++) {
          this.patchUniforms.push(this.buffer(16,Buffer.UNIFORM,new Float32Array([i,0,0,0])));
        }

        // Metadata packs sample-space origin, world-space height bounds, and up
        // to eight error levels into three vec4s per patch (select.wgsl Metadata).
        const metadata = new Float32Array(asset.patches.length*12);
        asset.patches.forEach((patch,i) => {
          metadata.set([patch.x,patch.z,patch.min,patch.max],i*12);
          metadata.set(patch.errors,i*12 + 4);
        });
        const inputs = this.buffer(metadata.byteLength,Buffer.STORAGE,metadata);

        // First dispatch writes fractional LODs. The second reads all neighbors
        // and writes stitched descriptors plus commands into resident buffers.
        const details = this.buffer(asset.patches.length*4,Buffer.STORAGE);
        const ranges = Uint32Array.from(this.grid.ranges.flatMap(range => [range.first,range.count]));
        const rangeBuffer = this.buffer(ranges.byteLength,Buffer.STORAGE,ranges);
        const shader = this.own(device.createShader({source:computeSource,language:'wgsl',stage:'compute'}));
        this.shaders.push(shader);
        for(const entryPoint of ['evaluate','reconcile']) {
          const bindings = (([
            {name:'scene',type:'uniform',group:0,location:0},
            {name:'metadata',type:'read-only-storage',group:0,location:1},
            {name:'details',type:'storage',group:0,location:2}
          ]) as BindingDeclaration[]);
          if(entryPoint === 'reconcile') {
            bindings.push({name:'patches',type:'storage',group:0,location:3},
              {name:'draws',type:'storage',group:0,location:4},
              {name:'ranges',type:'read-only-storage',group:0,location:5});
          }
          // Native auto layouts omit resources unused by this entry point. The
          // host layout must omit them too; whole-module reflection includes all.
          const pipeline = this.own(device.createComputePipeline({shader,entryPoint,shaderLayout:{bindings}}));
          pipeline.setBindings(entryPoint === 'evaluate' ? {scene:this.scene,metadata:inputs,details} : {scene:this.scene,metadata:inputs,details,patches:((this.descriptors) as Buffer),draws:((this.indirect) as Buffer),ranges:rangeBuffer});
          this.compute.push(pipeline);
        }
      } else {
        // Each WebGL draw needs its own uniform buffer: updating a single buffer
        // repeatedly before drawing would leave every patch with the last record.
        for(const patch of asset.patches) {
          this.patchUniforms.push(this.buffer(32,Buffer.UNIFORM));
        }
      }
    } catch(error) {
      // Release successfully created resources even if a later allocation fails.
      this.destroy();
      throw error;
    }
  }

  /**
   * Await pipeline validation before first use; native WebGPU pipeline creation
   * validates shader entry points. Poll its status rather than requesting duplicate
   * compilation-info promises (the adapter already owns those diagnostics).
   */
  async ready() {
    if(this.device.type === GPU_BACKEND.WEBGL) {
      const statuses = await Promise.all(this.shaders.map(shader => shader.asyncCompilationStatus));
      if(statuses.includes('error')) { throw new Error('Terrain GLSL compilation failed.'); }
      // WebGL's wrapper synchronizes the shared program link status at draw time.
      return;
    }
    while(this.pipeline.linkStatus === 'pending' && !this.disposed) {
      await new Promise(resolve => setTimeout(resolve,0));
    }
    if(this.pipeline.linkStatus === 'error') { throw new Error('Terrain render pipeline validation failed.'); }
  }

  /**
   * Update selection/morphs and enqueue compute on WebGPU. Never reads GPU data.
   * Matrix uses backend clip convention; projectionScale uses CSS pixels, so DPI
   * does not change render quality. CPU timing includes selection/submission only.
   * @param matrix
   * @param view
   * @param [diagnostic] 0 material, 1 LOD colors, 2 grid overlay.
   */
  prepare(matrix: ArrayLike<number>, view: TerrainView, diagnostic: number = 0) {
    if(this.disposed) { throw new Error('Terrain renderer disposed.'); }
    validateTerrainView(view);
    if(matrix.length !== 16 || !Array.from(matrix).every(Number.isFinite)) { throw new Error('Invalid terrain matrix.'); }
    const start = performance.now();
    const {size,tileCells,extent,heightOffset,heightScale} = this.asset.manifest;

    // Scene layout: matrix [0..15], eye + CSS projection scale [16..19],
    // sample size + tile cells + X/Z extents [20..23], height offset/scale +
    // tolerance + diagnostic mode [24..27], world target + pin radius [28..31].
    this.frameData.set(Array.from(matrix),0);
    this.frameData.set([...view.eye,view.projectionScale],16);
    this.frameData.set([size,tileCells,...extent],20);
    this.frameData.set([heightOffset,heightScale,view.tolerance,diagnostic],24);
    this.frameData.set([...view.target,view.targetRadius],28);
    this.scene.write(this.frameData);

    if(this.device.type === GPU_BACKEND.WEBGPU) {
      // Separate ordered passes ensure all LODs exist before any invocation reads
      // a neighbor. A workgroup-local barrier cannot synchronize the whole grid.
      for(const pipeline of this.compute) {
        const pass = this.device.beginComputePass();
        pass.setPipeline(pipeline);
        pass.dispatch(Math.ceil(this.asset.patches.length/64));
        pass.end();
      }
      // Avoid readback in the frame loop; the UI labels this as an upper bound.
      this.stats.triangles = this.stats.maxTriangles;
    } else {
      // CPU fallback applies the same selection and edge policy as compute.
      this.selection = selectTerrain(this.asset,view);
      this.stats.triangles = 0;
      this.selection.forEach((selection,i) => {
        const patch = this.asset.patches[i];
        // Patch layout: sample origin X/Z, fractional LOD, padding, then edge
        // LODs in -X, +X, -Z, +Z order. Integer LOD selects the index range.
        this.patchData.set([patch.x,patch.z,selection.detail,0,...selection.edges]);
        this.patchUniforms[i].write(this.patchData);
        this.stats.triangles+= this.grid.ranges[selection.lod].count/3;
      });
      this.stats.frameUploadBytes = 128 + this.patchUniforms.length*32;
    }

    // Submission cost and last completed GPU sample measure different work;
    // the asynchronous timer excludes selection compute and may lag this frame.
    this.stats.cpuMilliseconds = performance.now() - start;
    this.stats.gpuMilliseconds = this.timing.milliseconds;
    this.stats.gpuTiming = this.timing.status;
  }

  /**
   * Draw into a borrowed pass; does not clear/submit or own its framebuffer.
   * @param pass
   */
  draw(pass: RenderPass) {
    pass.setPipeline(this.pipeline);
    pass.setVertexArray(this.vertexArray);

    // Bind a descriptor ID per draw, then let the GPU-authored command choose
    // the index range. Geometry and height texture are shared across every leaf.
    if(this.descriptors && this.indirect) {
      for(let i = 0; i < this.asset.patches.length; i++) {
        // firstInstance remains zero, avoiding the optional indirect-first-instance feature.
        pass.setBindings({scene:this.scene,heightTexture:this.heightTexture,patches:this.descriptors,drawIndex:this.patchUniforms[i]});
        pass.drawIndexedIndirect(this.indirect,i*20);
      }
    } else {
      this.selection.forEach((selection,i) => {
        pass.setBindings({scene:this.scene,heightTexture:this.heightTexture,patchUniforms:this.patchUniforms[i]});
        const range = this.grid.ranges[selection.lod];
        // luma WebGL indexed draws use firstVertex as a byte offset; WebGPU uses firstIndex.
        pass.draw({indexCount:range.count,firstVertex:range.first*4});
      });
    }
  }

  /**
   * Release only owned resources; repeated disposal is harmless.
   */
  destroy() {
    if(this.disposed) { return; }
    this.disposed = true;

    // Reverse allocation order releases dependent objects before their inputs.
    // The borrowed device and CPU heightfield remain the caller's responsibility.
    for(const resource of this.resources.reverse()) { resource.destroy(); }
    this.resources.length = 0;
  }

  /**
   * Register owned resources immediately so constructor rollback can release them.
   * @private @param resource */
  private own<T extends {
    destroy(): void;
}>(resource: T): T { this.resources.push(resource); return resource; }

  /**
   * Allocate a tracked buffer with upload support; static data is written once.
   * @private @param byteLength @param usage @param [data]
   */
  private buffer(byteLength: number, usage: number, data?: Float32Array | Uint32Array) {
    const buffer = this.own(this.device.createBuffer({byteLength,usage:usage | Buffer.COPY_DST,...((usage & Buffer.INDEX) ? {indexType:'uint32'} : {})}));
    if(data) { buffer.write(data); }
    this.stats.residentGpuBytes+= byteLength;
    return buffer;
  }
}
