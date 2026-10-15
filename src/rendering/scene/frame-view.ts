// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {
  AtmosphereLight,
  EnvironmentFrame
} from '../../environment/environment-system.ts';
import type { Vector3 } from '../../core/types.ts';
import type { PointLight } from '../lighting/point-lights.ts';
import type {
  ShadowCameraFrame,
  ShadowOptions
} from '../shadows/types.ts';

import { DEFAULT_SHADOW_OPTIONS } from '../shadows/types.ts';
import { invertMatrix } from '../camera/projection.ts';
import {
  jitterProjection,
  multiplyCameraMatrices
} from './scene-camera-frame.ts';

/**
 * Read-only CPU data only; GPU resources and mutable owners never belong in this snapshot.
 */
type Immutable<T> = {
  readonly [Key in keyof T]: Immutable<T[Key]>
};

/**
 * Column-major camera snapshots. Unjittered matrices drive culling and shadow fitting.
 */
export interface FrameCamera {
  readonly viewMatrix: readonly number[];
  readonly projectionMatrix: readonly number[];
  readonly viewProjectionMatrix: readonly number[];
  readonly rasterProjectionMatrix: readonly number[];
  readonly rasterViewProjectionMatrix: readonly number[];
  readonly inverseViewMatrix: readonly number[] | null;
  readonly inverseProjectionMatrix: readonly number[] | null;
  readonly inverseViewProjectionMatrix: readonly number[] | null;
  readonly inverseRasterViewProjectionMatrix: readonly number[] | null;
  readonly nearPlane: number;
  readonly farPlane: number;
  readonly eye: Readonly<Vector3>;
  readonly backward: Readonly<Vector3>;
  /** Top-left UV units. The inverse projection reconstructs jittered raster depth. */
  readonly jitter: readonly [number, number];
}

/**
 * CSS dimensions preserve the view; width and height are physical backing pixels.
 */
export interface FrameViewport {
  readonly cssWidth: number;
  readonly cssHeight: number;
  readonly pixelRatio: number;
  readonly width: number;
  readonly height: number;
}

/**
 * One owned, deeply immutable CPU snapshot per scene submission. Matrices, light records,
 * environment, and shadow settings are copied before any feature runs. World space is Y-up,
 * distances are world units, and lighting is linear RGB. No render target or pass override
 * belongs here; features retain this same snapshot for the duration of the submission.
 */
export interface FrameView {
  readonly camera: FrameCamera;
  readonly viewport: FrameViewport;
  readonly lighting: {
    readonly lights: readonly Readonly<PointLight>[];
    readonly ambient: number;
    readonly directional: Immutable<AtmosphereLight> | null;
  };
  readonly environment: Immutable<EnvironmentFrame> | null;
  readonly shadows: Readonly<ShadowOptions> | false;
}

/**
 * Borrowed CPU inputs copied during view creation; subsequent caller edits cannot alter the view.
 */
export interface FrameViewOptions {
  camera: ShadowCameraFrame & {
    eye: readonly number[];
    backward: readonly number[];
  };
  viewport: {
    cssWidth: number;
    cssHeight: number;
    pixelRatio: number;
  };
  jitter?: readonly [number, number];
  lights?: readonly PointLight[];
  ambient?: number;
  environment?: EnvironmentFrame;
  /** Explicit direct light for low-level scenes without an atmosphere. Omission uses the environment. */
  directional?: AtmosphereLight | null;
  shadows?: ShadowOptions | false;
}

/**
 * Copy borrowed configuration once, compute shared camera transforms, and freeze all CPU data.
 * Plain frozen arrays deliberately replace typed arrays: a readonly typed-array wrapper still
 * exposes mutating methods and a writable backing buffer. GPU upload buffers remain pass-owned.
 * @param options Borrowed camera, viewport, lighting, environment, and shadow configuration.
 * @returns An owned immutable view; singular camera transforms have null inverse matrices.
 */
export function createFrameView(options: FrameViewOptions): FrameView {
  const source = options.camera;
  const viewMatrix = new Float32Array(source.viewMatrix);
  const projectionMatrix = new Float32Array(source.projectionMatrix);
  const jitter: [number, number] = [...(options.jitter ?? [0, 0])];
  const rasterProjection = jitterProjection(projectionMatrix, jitter);
  const viewProjection = multiplyCameraMatrices(projectionMatrix, viewMatrix);
  const rasterViewProjection = multiplyCameraMatrices(rasterProjection, viewMatrix);
  const environment = options.environment ? freezeData(structuredClone(options.environment)) : null;
  let directional: Immutable<AtmosphereLight> | null = null;
  if(options.directional === undefined) {
    directional = environment?.primaryLight ?? null;
  } else if(options.directional) {
    directional = freezeData(structuredClone(options.directional));
  }

  return Object.freeze({
    camera: Object.freeze({
      viewMatrix: freezeMatrix(viewMatrix),
      projectionMatrix: freezeMatrix(projectionMatrix),
      viewProjectionMatrix: freezeMatrix(viewProjection),
      rasterProjectionMatrix: freezeMatrix(rasterProjection),
      rasterViewProjectionMatrix: freezeMatrix(rasterViewProjection),
      inverseViewMatrix: freezeInverse(viewMatrix),
      inverseProjectionMatrix: freezeInverse(rasterProjection),
      inverseViewProjectionMatrix: freezeInverse(viewProjection),
      inverseRasterViewProjectionMatrix: freezeInverse(rasterViewProjection),
      nearPlane: source.nearPlane,
      farPlane: source.farPlane,
      eye: Object.freeze([source.eye[0], source.eye[1], source.eye[2]] as const),
      backward: Object.freeze([source.backward[0], source.backward[1], source.backward[2]] as const),
      jitter: Object.freeze(jitter)
    }),
    viewport: Object.freeze({
      ...options.viewport,
      width: Math.round(options.viewport.cssWidth*options.viewport.pixelRatio),
      height: Math.round(options.viewport.cssHeight*options.viewport.pixelRatio)
    }),
    lighting: Object.freeze({
      lights: Object.freeze((options.lights ?? []).map(light => Object.freeze({...light}))),
      ambient: options.ambient ?? (environment ? 0 : 0.3),
      directional
    }),
    environment,
    shadows: options.shadows === false ? false : Object.freeze({...DEFAULT_SHADOW_OPTIONS, ...options.shadows})
  });
}

/**
 * Copy a borrowed matrix into an immutable numeric array without a writable backing buffer.
 * @param matrix Borrowed column-major matrix; never modified.
 * @returns An owned frozen copy of every matrix element.
 */
function freezeMatrix(matrix: ArrayLike<number>): readonly number[] {
  return Object.freeze(Array.from(matrix));
}

/**
 * Invert a borrowed matrix and freeze the resulting CPU data.
 * @param matrix Borrowed column-major matrix; never modified.
 * @returns An owned immutable inverse, or null when the matrix is singular.
 */
function freezeInverse(matrix: ArrayLike<number>): readonly number[] | null {
  const inverse = invertMatrix(matrix);
  return inverse ? Object.freeze(inverse) : null;
}

/**
 * Freeze only cloned environment records and vectors, never borrowed objects or GPU resources.
 * @param value Owned cloned data whose nested objects may be frozen in place.
 * @returns The same object with all reachable child records frozen.
 */
function freezeData<T extends object>(value: T): Immutable<T> {
  for(const child of Object.values(value)) {
    if(child && typeof child === 'object') {
      freezeData(child);
    }
  }
  return Object.freeze(value);
}
