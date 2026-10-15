// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {
  Binding,
  Device
} from '@luma.gl/core';
import type {
  ShadowCamera,
  ShadowRenderView,
  ShadowShaderProps
} from '@luma.gl/experimental';
import type { AtmosphereLight } from '../../environment/environment-system.ts';
import type { PointLight } from '../lighting/point-lights.ts';
import type {
  PassCamera,
  RenderPassContext
} from '../scene/render-pass-context.ts';
import type { ShadowLightBatch } from './shadow-light-batches.ts';
import type {
  ShadowCameraFrame,
  ShadowOptions
} from './types.ts';

import { UniformStore } from '@luma.gl/core';
import { ShaderInputs } from '@luma.gl/engine';
import {
  ShadowMapRenderer,
  shadow
} from '@luma.gl/experimental';
import { createRenderPassContext } from '../scene/render-pass-context.ts';
import { prepareShadowLightBatches } from './shadow-light-batches.ts';
import { DEFAULT_SHADOW_OPTIONS } from './types.ts';

/**
 * ShadowPassContext Interface Definition.
 *  Frozen light-selection metadata and camera snapshot for one borrowed shadow pass.
 *  The callback records draws; the renderer retains ownership of the pass and target.
 */
export interface ShadowPassContext extends RenderPassContext {
  readonly camera: PassCamera;
  readonly type: ShadowRenderView['type'];
  readonly lightIndex: number;
  readonly cascadeIndex?: number;
  readonly pointFace?: ShadowRenderView['pointFace'];
}

/**
 * PreparedShadowBatch Interface Definition.
 *  CPU light-selection data with borrowed, instance-owned GPU lighting bindings.
 *  Bindings remain valid through the current frame submission; callers must not destroy them.
 */
export interface PreparedShadowBatch extends ShadowLightBatch {
  bindings: Record<string, Binding>;
}

interface ShadowBatchOwner {
  renderer: ShadowMapRenderer;
  inputs: ShaderInputs;
  store: UniformStore;
}

/**
 * SceneShadows Class Definition.
 *  Owns luma light-space maps and one uniform snapshot per lighting batch.
 *  Borrowed caster callbacks record draws; only the owning frame renderer submits.
 */
export class SceneShadows {
  // Owned resources, one independent set per active light batch.
  private readonly owners: ShadowBatchOwner[] = [];

  // Borrowed device; its creator releases it after this owner is destroyed.
  private readonly device: Device;

  // Copied configuration and mutable frame statistics.
  readonly options: ShadowOptions;
  readonly stats = {
    available: true,
    castingPointLights: 0,
    castingDirectionalLights: 0,
    batches: 0,
    views: 0
  };
  private disposed = false;

  /**
   * Configure independent WebGPU shadow resources, allocated on the first preparation.
   * @param device Borrowed WebGPU device, valid until destroy() completes.
   * @param options Borrowed settings copied over defaults; the input record is unchanged.
   * @throws If the backend is not WebGPU, or a configured bias or distance is invalid.
   */
  constructor(device: Device, options: ShadowOptions = {}) {
    if(device.type !== 'webgpu') {
      throw new Error('Geometric light shadows require WebGPU.');
    }

    this.device = device;
    this.options = {
      ...DEFAULT_SHADOW_OPTIONS,
      ...options
    };

    for(const key of ['normalBias', 'depthBias', 'depthBiasSlopeScale', 'shadowDistance'] as const) {
      const value = this.options[key]!;
      if(!Number.isFinite(value) || (value < 0)) {
        throw new RangeError(`Shadow ${key} must be finite and nonnegative.`);
      }
    }
  }

  /**
   * Record independent light views and snapshot their lighting uniforms without submitting.
   * Camera matrices and input lights are unchanged; statistics and owned GPU resources update.
   * @param camera Borrowed column-major view and WebGPU projection matrices, with world-unit clip distances.
   * @param lights Borrowed world-space lights; their order is retained across batches.
   * @param directional Borrowed world-space directional light, or null; only batch zero receives it.
   * @param drawCasters Synchronous recorder receiving a frozen context valid during its callback; it must not end the pass.
   * @returns New batch records borrowing owned bindings valid through this frame's submission.
   * @throws If destroyed, light capacity is exceeded, or resource allocation or caster recording fails.
   */
  prepare(
    camera: ShadowCameraFrame,
    lights: readonly PointLight[],
    directional: AtmosphereLight | null,
    drawCasters: (context: ShadowPassContext) => void): PreparedShadowBatch[] {
    if(this.disposed) {
      throw new Error('SceneShadows is destroyed.');
    }

    const groups = prepareShadowLightBatches(lights);
    const result: PreparedShadowBatch[] = [];
    this.stats.views = 0;
    this.stats.castingPointLights = 0;
    this.stats.castingDirectionalLights = 0;

    const shadowCamera: ShadowCamera = {
      viewMatrix: Array.from(camera.viewMatrix) as ShadowCamera['viewMatrix'],
      projectionMatrix: Array.from(camera.projectionMatrix) as ShadowCamera['projectionMatrix'],
      near: camera.nearPlane,
      far: camera.farPlane
    };

    for(const [index, group] of groups.entries()) {
      result.push(this.prepareBatch(index, group, shadowCamera, directional, drawCasters));
    }

    this.releaseInactiveBatches(groups.length);
    this.stats.batches = groups.length;
    return result;
  }

  /**
   * Update receiver scalars without reallocating light maps or caster pipelines.
   * @param options Borrowed settings; omitted receiver fields reset to defaults. Other fields are ignored.
   * @throws If a receiver value is not finite and nonnegative; earlier fields may already be updated.
   */
  setReceiverOptions(options: ShadowOptions) {
    for(const key of ['normalBias', 'shadowDistance'] as const) {
      const value = options[key] ?? DEFAULT_SHADOW_OPTIONS[key];
      if(!Number.isFinite(value) || (value < 0)) {
        throw new RangeError(`Shadow ${key} must be finite and nonnegative.`);
      }
      this.options[key] = value;
    }
  }

  /**
   * Release maps and uniforms before the borrowed device; repeated calls are harmless.
   * Previously returned GPU bindings become invalid. The device remains caller-owned.
   */
  destroy() {
    if(this.disposed) {
      return;
    }
    this.disposed = true;

    for(const owner of this.owners.splice(0)) {
      owner.store.destroy();
      owner.inputs.destroy();
      owner.renderer.destroy();
    }
  }

  /**
   * Record one light group and update its independent receiver bindings and frame statistics.
   * @param index Batch index; zero owns the directional light.
   * @param group Borrowed selected lights and indices, unchanged by preparation.
   * @param camera Copied frame camera with world-unit clip distances, unchanged by preparation.
   * @param directional Borrowed directional light, or null.
   * @param drawCasters Borrowed synchronous recorder; errors propagate to the caller.
   * @returns A new group record borrowing this batch owner's GPU resources.
   */
  private prepareBatch(
    index: number,
    group: ShadowLightBatch,
    camera: ShadowCamera,
    directional: AtmosphereLight | null,
    drawCasters: (context: ShadowPassContext) => void): PreparedShadowBatch {
    const owner = this.owners[index] ?? this.createOwner(index);
    const pointLights = group.lights.filter(light => light.castsShadow !== false).map(light => ({
      position: [light.x, light.y, light.z] as [number, number, number],
      range: light.range,
      nearPlane: Math.min(0.1, light.range*0.01),
      ...this.bias()
    }));
    const hasDirectionalShadow = (index === 0) && directional && (directional.castsShadow !== false) && (directional.intensity > 0);
    const directionalLights = hasDirectionalShadow ? [{
      direction: directional.direction,
      sourceAngularRadius: directional.angularRadius,
      shadowDistance: Math.min(this.options.shadowDistance!, camera.far),
      ...this.bias()
    }] : [];

    const props = owner.renderer.render({
      camera,
      pointLights,
      directionalLights,
      drawShadowCasters: view => this.recordCasterView(view, drawCasters)
    });

    // Match receiver projection depth to the converted raster views.
    props.directionalViewProjectionMatrices = props.directionalViewProjectionMatrices.map(toWebGpuDepth);
    props.pointViewProjectionMatrices = props.pointViewProjectionMatrices.map(toWebGpuDepth);
    props.spotViewProjectionMatrices = props.spotViewProjectionMatrices.map(toWebGpuDepth);
    owner.inputs.setProps({ shadow: props });
    owner.store.setUniforms(owner.inputs.getUniformValues());

    const bindings: Record<string, Binding> = {
      shadow: owner.store.getManagedUniformBuffer('shadow'),
      directionalShadowTexture: props.directionalShadowTexture,
      spotShadowTexture: props.spotShadowTexture,
      pointShadowTexture: props.pointShadowTexture,
      shadowComparisonSampler: props.comparisonSampler
    };

    this.stats.castingPointLights+= pointLights.length;
    this.stats.castingDirectionalLights+= directionalLights.length;
    return {
      ...group,
      bindings
    };
  }

  /**
   * Convert one light camera to WebGPU clip depth and invoke its caster recorder.
   * @param view Borrowed luma view and live render pass; neither is mutated or ended here.
   * @param drawCasters Synchronous callback borrowing the frozen context for this view.
   */
  private recordCasterView(
    view: ShadowRenderView,
    drawCasters: (context: ShadowPassContext) => void): void {
    this.stats.views++;

    // Luma 9.4.2 computes OpenGL-depth light projections. Convert both
    // raster and receiver matrices so WebGPU retains the entire light frustum.
    const camera: PassCamera = Object.freeze({
      viewMatrix: Object.freeze(Array.from(view.camera.viewMatrix)),
      projectionMatrix: Object.freeze(toWebGpuDepth(view.camera.projectionMatrix)),
      viewProjectionMatrix: Object.freeze(toWebGpuDepth(view.viewProjectionMatrix)),
      nearPlane: view.camera.near,
      farPlane: view.camera.far
    });
    const context = createRenderPassContext(view.renderPass, view.renderPass.props.framebuffer!, camera);

    drawCasters(Object.freeze({
      ...context,
      camera,
      type: view.type,
      lightIndex: view.lightIndex,
      cascadeIndex: view.cascadeIndex,
      pointFace: view.pointFace
    }));
  }

  /**
   * Release inactive groups after the active light population shrinks.
   * @param activeCount Number of leading owners to retain; removed bindings become invalid.
   */
  private releaseInactiveBatches(activeCount: number): void {
    for(const owner of this.owners.splice(activeCount)) {
      owner.store.destroy();
      owner.renderer.destroy();
      owner.inputs.destroy();
    }
  }

  /**
   * Copy configured receiver and raster bias values into a new light-settings record.
   * @returns Caller-owned scalar settings; mutating them does not update this owner.
   */
  private bias() {
    return {
      normalBias: this.options.normalBias,
      depthBias: this.options.depthBias,
      depthBiasSlopeScale: this.options.depthBiasSlopeScale
    };
  }

  /**
   * Allocate and retain one independent batch, releasing partial allocations on failure.
   * @param index Batch index; only zero allocates directional-map capacity.
   * @returns Instance-owned resources; callers borrow them and must not destroy them.
   * @throws If GPU or uniform allocation fails.
   */
  private createOwner(index: number): ShadowBatchOwner {
    const renderer = new ShadowMapRenderer(this.device, {
      quality: this.options.quality,
      pointLightCapacity: 4,
      spotLightCapacity: 0,
      spotMapSize: 1,
      directionalLightCapacity: index === 0 ? 1 : 0,
      directionalMapSize: index === 0 ? this.options.directionalMapSize : 1,
      pointMapSize: this.options.pointMapSize
    });
    let inputs: ShaderInputs | undefined;
    let store: UniformStore | undefined;

    try {
      inputs = new ShaderInputs({ shadow });
      store = new UniformStore(this.device, { shadow });
      store.getManagedUniformBuffer('shadow');

      const owner = { renderer, inputs, store };
      this.owners.push(owner);
      return owner;
    } catch(error) {
      store?.destroy();
      inputs?.destroy();
      renderer.destroy();
      throw error;
    }
  }
}

/**
 * Remap homogeneous clip Z from [-W,W] to [0,W], keeping X/Y/W unchanged.
 * @param matrix Borrowed column-major 4x4 matrix; its values are unchanged.
 * @returns A new column-major matrix suitable for WebGPU raster and receiver depth.
 */
function toWebGpuDepth(matrix: ArrayLike<number>): ShadowShaderProps['pointViewProjectionMatrices'][number] {
  const result = Array.from(matrix);
  for(let column = 0; column < 4; column++) {
    const offset = column*4;
    result[offset + 2] = (result[offset + 2] + result[offset + 3])*0.5;
  }
  return result as ShadowShaderProps['pointViewProjectionMatrices'][number];
}
