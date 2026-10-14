// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {UniformStore} from '@luma.gl/core';
import type {Binding, Device} from '@luma.gl/core';
import {ShaderInputs} from '@luma.gl/engine';
import {ShadowMapRenderer, shadow} from '@luma.gl/experimental';
import type {ShadowRenderView, ShadowShaderProps} from '@luma.gl/experimental';
import type {PointLight} from '../lighting/point-lights.ts';
import type {AtmosphereLight} from '../../environment/environment-system.ts';
import {prepareShadowLightBatches} from './shadow-light-batches.ts';
import type {ShadowLightBatch} from './shadow-light-batches.ts';
import {DEFAULT_SHADOW_OPTIONS} from './types.ts';
import type {ShadowCameraFrame, ShadowOptions} from './types.ts';

export interface PreparedShadowBatch extends ShadowLightBatch {
  bindings: Record<string, Binding>;
}

/**
 * Owns luma light-space maps and one immutable uniform snapshot per lighting batch.
 * Borrowed caster callbacks record draws; only the owning frame renderer submits.
 */
export class SceneShadows {
  private readonly owners: {renderer: ShadowMapRenderer; inputs: ShaderInputs; store: UniformStore}[] = [];
  private disposed = false;
  private readonly device: Device;
  readonly options: ShadowOptions;
  readonly stats = {available: true, castingPointLights: 0, castingDirectionalLights: 0, batches: 0, views: 0};

  constructor(device: Device, options: ShadowOptions = {}) {
    if(device.type !== 'webgpu') {
      throw new Error('Geometric light shadows require WebGPU.');
    }
    this.device = device;
    this.options = {...DEFAULT_SHADOW_OPTIONS, ...options};
    for(const key of ['normalBias', 'depthBias', 'depthBiasSlopeScale', 'shadowDistance'] as const) {
      const value = this.options[key]!;
      if(!Number.isFinite(value) || value < 0) {
        throw new RangeError(`Shadow ${key} must be finite and nonnegative.`);
      }
    }
  }

  /**
   * Record independent light views and return borrowed lighting bindings.
   * Camera matrices use WebGPU clip depth; lights and biases are world-space.
   */
  prepare(camera: ShadowCameraFrame, lights: readonly PointLight[], directional: AtmosphereLight | null,
    drawCasters: (view: ShadowRenderView) => void): PreparedShadowBatch[] {
    if(this.disposed) {
      throw new Error('SceneShadows is destroyed.');
    }
    const groups = prepareShadowLightBatches(lights);
    const result: PreparedShadowBatch[] = [];
    this.stats.views = 0;
    this.stats.castingPointLights = 0;
    this.stats.castingDirectionalLights = 0;
    const shadowCamera = {
      viewMatrix: Array.from(camera.viewMatrix) as [number, number, number, number, number, number, number, number, number, number, number, number, number, number, number, number],
      projectionMatrix: Array.from(camera.projectionMatrix) as [number, number, number, number, number, number, number, number, number, number, number, number, number, number, number, number],
      near: camera.nearPlane, far: camera.farPlane
    };
    for(const [index, group] of groups.entries()) {
      const owner = this.owners[index] ?? this.createOwner(index);
      const pointLights = group.lights.filter(light => light.castsShadow !== false).map(light => ({
        position: [light.x, light.y, light.z] as [number, number, number], range: light.range,
        nearPlane: Math.min(0.1, light.range*0.01), ...this.bias()
      }));
      const directionalLights = index === 0 && directional && directional.castsShadow !== false && directional.intensity > 0 ? [{
        direction: directional.direction, sourceAngularRadius: directional.angularRadius,
        shadowDistance: Math.min(this.options.shadowDistance!, camera.farPlane), ...this.bias()
      }] : [];
      const props = owner.renderer.render({camera: shadowCamera, pointLights, directionalLights,
        drawShadowCasters: view => {
          this.stats.views++;
          // Luma 9.4.2 computes OpenGL-depth light projections. Convert both
          // raster and receiver matrices so WebGPU retains the entire light frustum.
          const matrix = view.viewProjectionMatrix.clone();
          matrix.copy(toWebGpuDepth(view.viewProjectionMatrix));
          drawCasters({...view, viewProjectionMatrix: matrix,
            camera: {...view.camera, projectionMatrix: toWebGpuDepth(view.camera.projectionMatrix)}});
        }
      });
      props.directionalViewProjectionMatrices = props.directionalViewProjectionMatrices.map(toWebGpuDepth);
      props.pointViewProjectionMatrices = props.pointViewProjectionMatrices.map(toWebGpuDepth);
      props.spotViewProjectionMatrices = props.spotViewProjectionMatrices.map(toWebGpuDepth);
      owner.inputs.setProps({shadow: props});
      owner.store.setUniforms(owner.inputs.getUniformValues());
      result.push({...group, bindings: {
        shadow: owner.store.getManagedUniformBuffer('shadow'),
        directionalShadowTexture: props.directionalShadowTexture,
        spotShadowTexture: props.spotShadowTexture,
        pointShadowTexture: props.pointShadowTexture,
        shadowComparisonSampler: props.comparisonSampler
      }});
      this.stats.castingPointLights+= pointLights.length;
      this.stats.castingDirectionalLights+= directionalLights.length;
    }
    // Release inactive groups when an effect's active light population shrinks.
    for(const owner of this.owners.splice(groups.length)) {
      owner.store.destroy();
      owner.renderer.destroy();
      owner.inputs.destroy();
    }
    this.stats.batches = groups.length;
    return result;
  }

  /** Release maps and uniforms before the borrowed device. Safe to repeat. */
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

  /** Update receiver-only scalars without reallocating light maps or caster pipelines. */
  setReceiverOptions(options: ShadowOptions) {
    for(const key of ['normalBias', 'shadowDistance'] as const) {
      const value = options[key] ?? DEFAULT_SHADOW_OPTIONS[key];
      if(!Number.isFinite(value) || value < 0) {
        throw new RangeError(`Shadow ${key} must be finite and nonnegative.`);
      }
      this.options[key] = value;
    }
  }

  private bias() {
    return {normalBias: this.options.normalBias, depthBias: this.options.depthBias,
      depthBiasSlopeScale: this.options.depthBiasSlopeScale};
  }

  private createOwner(index: number) {
    const renderer = new ShadowMapRenderer(this.device, {
      quality: this.options.quality, pointLightCapacity: 4, spotLightCapacity: 0, spotMapSize: 1,
      directionalLightCapacity: index === 0 ? 1 : 0,
      directionalMapSize: index === 0 ? this.options.directionalMapSize : 1,
      pointMapSize: this.options.pointMapSize
    });
    let inputs: ShaderInputs | undefined;
    let store: UniformStore | undefined;
    try {
      inputs = new ShaderInputs({shadow});
      store = new UniformStore(this.device, {shadow});
      store.getManagedUniformBuffer('shadow');
      const owner = {renderer, inputs, store};
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

/** Remap homogeneous clip Z from [-W,W] to [0,W], keeping X/Y/W unchanged. */
function toWebGpuDepth(matrix: ArrayLike<number>): ShadowShaderProps['pointViewProjectionMatrices'][number] {
  const result = Array.from(matrix);
  for(let column = 0; column < 4; column++) {
    const offset = column*4;
    result[offset + 2] = (result[offset + 2] + result[offset + 3])*0.5;
  }
  return result as [number, number, number, number, number, number, number, number, number, number, number, number, number, number, number, number];
}
