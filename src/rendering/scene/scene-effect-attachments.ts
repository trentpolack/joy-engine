// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {Device, Texture} from '@luma.gl/core';
import type {PointLight} from '../pbr/point-lights.ts';
import type {SceneEffectsFrame} from './scene-effects.ts';
import type {SceneMotionFrame} from './scene-motion.ts';
import {SceneMotionPass} from './scene-motion.ts';
import {SceneSurfaces} from './scene-surfaces.ts';
import {SceneVolumeLights} from './scene-volume-lights.ts';
import {getSceneEffectsRequirements} from './scene-effect-config.ts';

/** Borrowed CPU inputs; valid for one submission. Color/depth remain owned by the scene target. */
export interface SceneAttachmentFrame {
  motion: SceneMotionFrame;
  viewMatrix: Float32Array<ArrayBuffer>;
  surfaceRoughness: readonly number[];
  lights: readonly PointLight[];
}

/** Owns optional auxiliary attachments and releases them when their last consumer is disabled. */
export class SceneEffectAttachments {
  private motion: SceneMotionPass | null = null;
  private surfaces: SceneSurfaces | null = null;
  private volume: SceneVolumeLights | null = null;

  private device: Device;

  constructor(device: Device) {
    this.device = device;
  }

  prepare(requirements: ReturnType<typeof getSceneEffectsRequirements>, depth: Texture, frame: SceneEffectsFrame, inputs: SceneAttachmentFrame): SceneEffectsFrame {
    const result: SceneEffectsFrame = {...frame, depthTexture: depth};
    if(requirements.velocityTexture || requirements.taaVelocityTexture) {
      this.motion ??= new SceneMotionPass(this.device);
      result.taaVelocityTexture = this.motion.render(inputs.motion);
      result.velocityTexture = this.motion.jitteredVelocityTexture ?? undefined;
    } else {
      this.motion?.destroy();
      this.motion = null;
    }
    if(requirements.normalTexture) {
      this.surfaces ??= new SceneSurfaces(this.device);
      result.normalTexture = this.surfaces.render(inputs.motion.vertices as number[] | Float32Array,
        new Float32Array(inputs.motion.rasterViewProjection ?? inputs.motion.currentViewProjection),
        inputs.viewMatrix, depth, inputs.surfaceRoughness);
    } else {
      this.surfaces?.destroy();
      this.surfaces = null;
    }
    if(requirements.clusteredLighting) {
      if(!frame.projectionMatrix || frame.nearPlane === undefined || frame.farPlane === undefined) {
        throw new Error('Clustered lighting requires a complete perspective camera snapshot.');
      }
      this.volume ??= new SceneVolumeLights(this.device);
      result.clusteredLighting = this.volume.update(inputs.lights, {
        viewMatrix: inputs.viewMatrix,
        projectionMatrix: frame.projectionMatrix,
        nearPlane: frame.nearPlane, farPlane: frame.farPlane, width: depth.width, height: depth.height
      });
    } else {
      this.volume?.destroy();
      this.volume = null;
    }
    return result;
  }

  get volumeStats() {
    return this.volume?.stats ?? null;
  }

  destroy() {
    this.motion?.destroy();
    this.surfaces?.destroy();
    this.volume?.destroy();
    this.motion = null;
    this.surfaces = null;
    this.volume = null;
  }
}
