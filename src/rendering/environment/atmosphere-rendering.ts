// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { EnvironmentFrame } from '../../environment/environment-system.ts';

import wgsl from '../../../shaders/rendering/environment/atmosphere.wgsl?raw';
import vertex from '../../../shaders/rendering/environment/atmosphere.vert.glsl?raw';
import fragment from '../../../shaders/rendering/environment/atmosphere.frag.glsl?raw';
import { createEnvironmentUniforms, ENVIRONMENT_UNIFORM_BYTES } from './environment-uniforms.ts';

export const ATMOSPHERE_RENDER_UNIFORM_BYTES = 352;

/**
 * Portable procedural sky program for the engine environment frame.
 *
 * The pass renders a full-screen triangle without a vertex buffer. It combines
 * Rayleigh/Mie atmosphere approximations, sun/moon discs, wind-advected cloud
 * coverage, and a forward-scattering halo that provides the first light-shaft
 * contribution. Scene-depth fog, occlusion-aware shafts, cloud shadows, and AO
 * remain separate scene passes because they need renderer-owned textures.
 */
export const ATMOSPHERE_SHADERS = Object.freeze({wgsl, glsl: {vertex, fragment}});

/**
 * Pack camera and environment state for ATMOSPHERE_SHADERS.
 * The inverse view-projection matrix is column-major. Camera position and cloud
 * distances are Y-up world units; viewport dimensions are physical pixels.
 * The returned array is caller-owned and contains no GPU resources.
 * @param inverseViewProjection
 * @param cameraPosition
 * @param viewportSize
 * @param frame
 */
export function createAtmosphereRenderUniforms(inverseViewProjection: ArrayLike<number>, cameraPosition: readonly [
    number,
    number,
    number
], viewportSize: readonly [
    number,
    number
], frame: EnvironmentFrame): Float32Array<ArrayBuffer> {
  if(inverseViewProjection.length !== 16) {
    throw new RangeError('Atmosphere rendering needs a 4x4 inverse view-projection matrix.');
  }
  if(cameraPosition.length !== 3 || viewportSize.length !== 2 || viewportSize[0] <= 0 || viewportSize[1] <= 0) {
    throw new RangeError('Atmosphere rendering needs a 3D camera position and positive viewport size.');
  }

  const data = new Float32Array(ATMOSPHERE_RENDER_UNIFORM_BYTES/Float32Array.BYTES_PER_ELEMENT);
  data.set(inverseViewProjection, 0);
  data.set([...cameraPosition, 0], 16);
  data.set([...viewportSize, 1/viewportSize[0], 1/viewportSize[1]], 20);
  data.set(createEnvironmentUniforms(frame), 24);
  if(!data.every(Number.isFinite)) {
    throw new RangeError('Atmosphere render uniforms must fit finite float32 values.');
  }
  return data;
}

