// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {Buffer, Device, RenderPass} from '@luma.gl/core';
import {SceneShadows} from './scene-shadows.ts';
import {ShadowCasters} from './shadow-casters.ts';
import type {SceneLightFrame, ShadowOptions} from './types.ts';

/** One frame-owned set of maps, shared by every geometry adapter and receiver. */
export class SceneShadowPass {
  readonly shadows: SceneShadows;
  private readonly casters: ShadowCasters;
  private disposed = false;

  constructor(device: Device, options: ShadowOptions) {
    this.shadows = new SceneShadows(device, options);
    try {
      this.casters = new ShadowCasters(device, options);
    } catch(error) {
      this.shadows.destroy();
      throw error;
    }
  }

  /** Record the complete caster set before camera-visible color draws. */
  prepare(frame: SceneLightFrame, drawNative?: (pass: RenderPass, matrix: Buffer) => void) {
    this.casters.prepare(frame.casters);
    return this.shadows.prepare(frame.view.camera, frame.view.lighting.lights, frame.view.lighting.directional,
      view => this.casters.draw(view, drawNative));
  }

  destroy() {
    if(this.disposed) {
      return;
    }
    this.disposed = true;
    this.casters.destroy();
    this.shadows.destroy();
  }
}
