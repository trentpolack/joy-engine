// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {shadow} from '@luma.gl/experimental';
import depthSource from '../../../shaders/rendering/shadows/shadow-depth.wgsl?raw';
import pointSource from '../../../shaders/rendering/shadows/shadow-point.wgsl?raw';

const POINT_REFERENCE_FUNCTION = /fn shadow_pointReferenceDepth\([^]*?\n\}/;
const POINT_FILTER_FUNCTION = /fn shadow_getPointFactor\([^]*?\n\}/;

/**
 * Luma 9.4.2 reconstructs OpenGL cube-reference depth. Our light raster matrices
 * use WebGPU depth, so every receiver must use the matching reconstruction.
 * Offset cube taps also need their own receiver-plane depth to avoid treating
 * the receiver itself as a blocker. Keep both corrections local; never mutate
 * luma globals. Revisit these adapters when updating the upstream dependency.
 */
function createSceneShadowModule() {
  if(!shadow.source || !POINT_REFERENCE_FUNCTION.test(shadow.source) || !POINT_FILTER_FUNCTION.test(shadow.source)) {
    throw new Error('The installed shadow module requires review of its WebGPU point-depth adapter.');
  }
  const source = shadow.source.replace(POINT_REFERENCE_FUNCTION, depthSource).replace(POINT_FILTER_FUNCTION, pointSource);
  return {...shadow, source};
}

export const sceneShadowModule = createSceneShadowModule();
