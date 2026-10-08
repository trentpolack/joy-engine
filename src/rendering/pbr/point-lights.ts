// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { EnvironmentFrame } from '../../environment/environment-system.ts';

import wgsl from '../../../shaders/rendering/lit-triangles/lit-triangles.wgsl?raw';
import vertex from '../../../shaders/rendering/lit-triangles/lit-triangles.vert.glsl?raw';
import fragment from '../../../shaders/rendering/lit-triangles/lit-triangles.frag.glsl?raw';

export interface PointLight {
  x: number;
  y: number;
  z: number;
  r: number;
  g: number;
  b: number;
  range: number;
}
export const MAX_POINT_LIGHTS = 32;
export const POINT_LIGHT_UNIFORM_BYTES = 1168;
/** Flat, double-sided diffuse lighting for XYZ/RGBA opaque triangles.
 * Normals come from surface derivatives; lights do not cast shadows.
 * Linear HDR output feeds the renderer's optional scene post processing.
 */
export const LIT_TRIANGLE_SHADERS = Object.freeze({ wgsl, glsl: { vertex, fragment } });

/** Own a camera/tint/light uniform snapshot for one frame. Distances are world units; RGB is linear irradiance.
 * A light smoothly reaches zero at its range.
 * Supply as the scene uniforms with LIT_TRIANGLE_SHADERS as opaqueShaderSources.
 * The first 80 bytes also match the unlit particle and textured sprite shaders.
 * @param matrix Column-major view/projection matrix.
 * @param lights Borrowed records, at most 32.
 * @param [ambient] Nonnegative ambient irradiance, added to optional skylight.
 * @param [environment] Borrowed immutable Y-up atmosphere lighting. Omitted means no directional light or colored skylight.
 * @returns A packed snapshot of the camera and lights for one frame, ready to upload to the GPU.
 */
export function createPointLightUniforms(matrix: ArrayLike<number>, lights: readonly PointLight[], ambient: number = 0.3, environment: EnvironmentFrame | null = null) {
  if(matrix.length !== 16 || lights.length > MAX_POINT_LIGHTS || !Number.isFinite(ambient) || ambient < 0) {
    throw new RangeError('Point light uniforms need a 4×4 matrix, nonnegative ambient and at most 32 lights.');
  }
  const data = new Float32Array(POINT_LIGHT_UNIFORM_BYTES / 4);
  for(let index = 0; index < 16; index++) {
    if(!Number.isFinite(matrix[index])) {
      throw new RangeError('Camera matrix must be finite.');
    }
    data[index] = matrix[index];
  }
  data.set([1, 1, 1, 1, lights.length, ambient, 0, 0], 16);
  lights.forEach((light, index) => {
    const values = [light.x, light.y, light.z, light.range, light.r, light.g, light.b, 0];
    if(values.some(value => !Number.isFinite(value)) || light.range <= 0 || Math.min(light.r, light.g, light.b) < 0) {
      throw new RangeError('Point light position/color must be finite and its range positive.');
    }
    data.set(values, 24 + index * 8);
  });
  if(environment) {
    const light = environment.primaryLight;
    data.set([...light.direction, light.intensity], 280);
    data.set([...light.color, 0], 284);
    data.set([...environment.skylight.color, environment.skylight.intensity], 288);
  }
  // JavaScript's finite double range exceeds the GPU's float32 range. Validate
  // the packed snapshot too so accepted inputs cannot become infinities or a
  // zero attenuation denominator during upload.
  if(!data.every(Number.isFinite) || lights.some((_, index) => data[27 + index * 8] <= 0)) {
    throw new RangeError('Point light uniforms must fit finite float32 values and preserve positive ranges.');
  }

  return data;
}
