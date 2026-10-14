// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {
  Device,
  Texture
} from '@luma.gl/core';
import type { FrameView } from './frame-view.ts';
import type { SceneEffectsFrame } from './scene-effects.ts';
import type { SceneMotionFrame } from './scene-motion.ts';
import type { SceneVolumeLightCamera } from '../lighting/scene-volume-lights.ts';
import type { getSceneEffectsRequirements } from './scene-effect-config.ts';
import type { NativeSceneGeometry } from './native-geometry.ts';

import { SceneMotionPass } from './scene-motion.ts';
import { SceneSurfaces } from './scene-surfaces.ts';
import { SceneVolumeLights } from '../lighting/scene-volume-lights.ts';

type SceneAttachmentRequirements = ReturnType<typeof getSceneEffectsRequirements>;

/**
 * SceneAttachmentFrame Interface Definition.
 *  Borrowed CPU inputs; valid for one submission.
 *  Color/depth remain owned by the scene target.
 */
export interface SceneAttachmentFrame {
  motion: SceneMotionFrame;
  readonly view: FrameView;
  surfaceRoughness: ArrayLike<number>;
  nativeGeometry: readonly NativeSceneGeometry[];
}

/**
 * SceneEffectAttachments Class Definition.
 *  Owns optional auxiliary attachments and releases them when their last consumer is disabled.
 */
export class SceneEffectAttachments {
  private motion: SceneMotionPass | null = null;
  private surfaces: SceneSurfaces | null = null;
  private volume: SceneVolumeLights | null = null;

  private readonly device: Device;

  /**
   * Borrow the device; optional attachment owners are allocated only when requested.
   * @param device Shared device whose lifetime must cover this attachment owner.
   */
  constructor(device: Device) {
    this.device = device;
  }

  /**
   * Release all owned attachments and destroy their render passes.
   */
  destroy() {
    this.motion?.destroy();
    this.surfaces?.destroy();
    this.volume?.destroy();
    this.motion = null;
    this.surfaces = null;
    this.volume = null;
  }

  /**
   * Prepare required attachments in motion, surface, and lighting order.
   * Borrows inputs and depth; returns a shallow frame copy with instance-owned attachments.
   * The caller's frame record and shared FrameView are unchanged.
   * @param requirements Attachments required by the enabled scene effects.
   * @param depth Scene-owned depth texture for surface and volume lighting.
   * @param frame SceneEffectsFrame to copy and update with owned attachments.
   * @param inputs SceneAttachmentFrame with borrowed inputs for motion, surface, and lighting.
   * @returns A new frame record borrowing the required attachments owned by this instance.
   */
  prepare(
    requirements: SceneAttachmentRequirements,
    depth: Texture,
    frame: SceneEffectsFrame,
    inputs: SceneAttachmentFrame): SceneEffectsFrame {
    const outputFrame: SceneEffectsFrame = {
      ...frame,
      depthTexture: depth
    };

    this.prepareMotionAttachments(requirements, inputs, outputFrame);
    this.prepareSurfaceAttachments(requirements, depth, inputs, outputFrame);
    this.prepareVolumeLighting(requirements, depth, inputs, outputFrame);

    return outputFrame;
  }

  /**
   * Volume lighting statistics accessor.
   * @returns The statistics for the volume lighting, or null if it is not initialized.
   */
  get volumeStats() {
    return(this.volume?.stats ?? null);
  }

  /**
   * Update both motion spaces on outputFrame, or release the pass when neither is needed.
   * @param requirements SceneEffectRequirements from the scene's effect config.
   * @param inputs SceneAttachmentFrame with borrowed motion inputs.
   * @param outputFrame SceneEffectsFrame to update with owned motion attachments.
   */
  private prepareMotionAttachments(
    requirements: SceneAttachmentRequirements,
    inputs: SceneAttachmentFrame,
    outputFrame: SceneEffectsFrame): void {
    if(!requirements.velocityTexture && !requirements.taaVelocityTexture) {
      this.motion?.destroy();
      this.motion = null;
      return;
    }

    this.motion ??= new SceneMotionPass(this.device);
    outputFrame.taaVelocityTexture = this.motion.render(inputs.motion, pass => {
      for(const geometry of inputs.nativeGeometry) {
        if(geometry.visible) {
          geometry.drawMotion(pass, {
            currentViewProjection: inputs.motion.currentViewProjection,
            previousViewProjection: inputs.motion.previousViewProjection,
            rasterViewProjection: inputs.motion.rasterViewProjection ?? inputs.motion.currentViewProjection,
            velocityJitterDelta: inputs.motion.velocityJitterDelta ?? [0, 0]
          });
        }
      }
    });
    outputFrame.velocityTexture = this.motion.jitteredVelocityTexture ?? undefined;
  }

  /**
   * Update outputFrame's surface attachment using borrowed scene depth, or release the pass.
   * @param requirements SceneEffectRequirements from the scene's effect config.
   * @param depth Texture with the scene's depth buffer.
   * @param inputs SceneAttachmentFrame with borrowed surface inputs.
   * @param outputFrame SceneEffectsFrame to update with owned surface attachments.
   */
  private prepareSurfaceAttachments(
    requirements: SceneAttachmentRequirements,
    depth: Texture,
    inputs: SceneAttachmentFrame,
    outputFrame: SceneEffectsFrame): void {
    if(!requirements.normalTexture) {
      this.surfaces?.destroy();
      this.surfaces = null;
      return;
    }

    this.surfaces ??= new SceneSurfaces(this.device);
    const rasterViewProjection = inputs.view.camera.rasterViewProjectionMatrix;

    outputFrame.normalTexture = this.surfaces.render(
      inputs.motion.vertices as number[] | Float32Array,
      rasterViewProjection,
      inputs.view.camera.viewMatrix,
      depth,
      inputs.surfaceRoughness,
      pass => {
        for(const geometry of inputs.nativeGeometry) {
          if(geometry.visible) {
            geometry.drawSurface(pass, rasterViewProjection, inputs.view.camera.viewMatrix);
          }
        }
      }
    );
  }

  /**
   * Update outputFrame's borrowed lighting bindings, or release their owned storage.
   * Camera and lights come from the same immutable view used by the scene passes.
   * @param requirements SceneEffectRequirements from the scene's effect config.
   * @param depth Texture with the scene's depth buffer.
   * @param inputs SceneAttachmentFrame with borrowed lighting inputs.
   * @param outputFrame SceneEffectsFrame to update with owned lighting attachments.
   */
  private prepareVolumeLighting(
    requirements: SceneAttachmentRequirements,
    depth: Texture,
    inputs: SceneAttachmentFrame,
    outputFrame: SceneEffectsFrame): void {
    if(!requirements.clusteredLighting) {
      this.volume?.destroy();
      this.volume = null;
      return;
    }

    this.volume ??= new SceneVolumeLights(this.device);

    // Use clip planes from the same snapshot as the frame's projection matrix.
    const camera: SceneVolumeLightCamera = {
      viewMatrix: inputs.view.camera.viewMatrix,
      projectionMatrix: inputs.view.camera.rasterProjectionMatrix,
      nearPlane: inputs.view.camera.nearPlane,
      farPlane: inputs.view.camera.farPlane,
      width: depth.width,
      height: depth.height
    };

    outputFrame.clusteredLighting = this.volume.update(inputs.view.lighting.lights, camera);
  }
}
