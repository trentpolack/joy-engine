// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { FrameView } from '../scene/frame-view.ts';

/**
 * ShadowOptions Interface Definition.
 *  Caller-owned configuration copied by shadow resource owners.
 *  Biases and distance must be finite and nonnegative when configuring SceneShadows.
 */
export interface ShadowOptions {
  /**
   * Shadow sampling quality preset; omitted uses the named default.
   */
  quality?: 'low' | 'balanced' | 'cinematic';
  /**
   * Directional depth-map width and height in pixels; omitted uses the backend quality preset.
   */
  directionalMapSize?: number;
  /**
   * Point-light cube-face width and height in pixels; omitted uses the backend quality preset.
   */
  pointMapSize?: number;
  /**
   * Receiver displacement along its world-space normal, in world units.
   */
  normalBias?: number;
  /**
   * Constant caster raster bias in backend depth units.
   */
  depthBias?: number;
  /**
   * Caster depth bias scaled by the depth slope; dimensionless.
   */
  depthBiasSlopeScale?: number;
  /**
   * Maximum directional shadow distance from the camera, in world units.
   */
  shadowDistance?: number;
}

/**
 * ShadowCameraFrame Interface Definition.
 *  Borrowed column-major world-to-view and WebGPU projection matrices, unchanged by preparation.
 *  Each matrix contains 16 values; clip distances are in world units and remain valid for the call.
 */
export interface ShadowCameraFrame {
  viewMatrix: ArrayLike<number>;
  projectionMatrix: ArrayLike<number>;
  nearPlane: number;
  farPlane: number;
}

/**
 * SceneLightFrame Interface Definition.
 *  Shared immutable view and borrowed caster geometry for one default lit-triangle preparation.
 *  Preparation copies geometry into owned GPU storage and leaves this record and its inputs unchanged.
 */
export interface SceneLightFrame {
  readonly view: FrameView;
  /**
   * Complete opaque world-space XYZ/RGBA caster triangles, including objects outside the camera frustum.
   * Input remains caller-owned and must be valid until preparation returns.
   */
  casters: number[] | Float32Array;
}

/**
 * Named immutable defaults keep editor-scale bias independent from world geometry size.
 * Resource owners copy these values; callers must not mutate the shared record.
 */
export const DEFAULT_SHADOW_OPTIONS = Object.freeze({
  quality: 'low' as const,
  normalBias: 0.01,
  depthBias: 2,
  depthBiasSlopeScale: 2,
  shadowDistance: 100
});
