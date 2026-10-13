// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {PointLight} from '../pbr/point-lights.ts';
import type {AtmosphereLight} from '../../environment/environment-system.ts';

/** Quality and bias in world units (normal bias), depth units, and world-space shadow distance. */
export interface ShadowOptions {
  quality?: 'low' | 'balanced' | 'cinematic';
  directionalMapSize?: number;
  pointMapSize?: number;
  normalBias?: number;
  depthBias?: number;
  depthBiasSlopeScale?: number;
  shadowDistance?: number;
}

/** Borrowed column-major world-to-view and WebGPU projection matrices; clip distances are world units. */
export interface ShadowCameraFrame {
  viewMatrix: ArrayLike<number>;
  projectionMatrix: ArrayLike<number>;
  nearPlane: number;
  farPlane: number;
}

/** Borrowed lighting/caster data for the default lit triangle pass. */
export interface SceneLightFrame {
  camera: ShadowCameraFrame;
  lights: readonly PointLight[];
  directional: AtmosphereLight | null;
  /** Complete opaque XYZ/RGBA caster triangles, including objects outside the camera frustum. */
  casters: number[] | Float32Array;
  /** Camera-visible opaque runs; omitted means every colored receiver accepts shadows. */
  receivesShadowRanges?: readonly {firstVertex: number; vertexCount: number; receivesShadow: boolean}[];
}

/** Named defaults keep editor-scale bias independent from world geometry size. */
export const DEFAULT_SHADOW_OPTIONS = Object.freeze({
  quality: 'low' as const,
  normalBias: 0.01,
  depthBias: 2,
  depthBiasSlopeScale: 2,
  shadowDistance: 100
});
