// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {Binding, Buffer, RenderPass} from '@luma.gl/core';

/** Borrowed resident geometry, prepared before any scene or light render pass opens. */
export interface NativeSceneGeometry {
  visible: boolean;
  castsShadow: boolean;
  receivesShadow: boolean;
  draw(pass: RenderPass, bindings: Record<string, Binding>, shadowed: boolean, additive: boolean): void;
  drawShadow(pass: RenderPass, matrix: Buffer): void;
  drawSurface(pass: RenderPass, viewProjection: ArrayLike<number>, viewMatrix: ArrayLike<number>): void;
  drawMotion(pass: RenderPass, frame: {
    currentViewProjection: ArrayLike<number>;
    previousViewProjection: ArrayLike<number>;
    rasterViewProjection: ArrayLike<number>;
    velocityJitterDelta: readonly [number, number];
  }): void;
}
