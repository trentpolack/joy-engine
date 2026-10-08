// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { SCENE_EFFECT, SCENE_REPROJECTION } from 'joy-engine/constants';
import type { SceneEffectsOptions } from 'joy-engine';

export const EFFECTS = [
  {key:SCENE_EFFECT.DEPTH_AWARE_BLUR, label:'Depth-aware blur', detail:'Softens surfaces while preserving depth edges.'},
  {key:SCENE_EFFECT.SSAO, label:'SSAO', detail:'Contact darkening near blocks and wall corners.'},
  {key:SCENE_EFFECT.GTAO, label:'GTAO', detail:'Alternative horizon-based ambient occlusion; replaces SSAO.'},
  {key:SCENE_EFFECT.SSGI, label:'SSGI', detail:'Screen-space color bleed from the red and green walls.'},
  {key:SCENE_EFFECT.OUTLINES, label:'Outlines', detail:'Depth and normal discontinuities around geometry.'},
  {key:SCENE_EFFECT.TAA, label:'TAA', detail:'Temporal antialiasing on thin slats and moving silhouettes.'},
  {key:SCENE_EFFECT.MOTION_BLUR, label:'Motion blur', detail:'Enable object or camera motion to see velocity trails.'},
  {key:SCENE_EFFECT.SSR, label:'SSR', detail:'Screen-space reflection on the smooth floor and blue block.'},
  {key:SCENE_EFFECT.HEIGHT_FOG, label:'Height fog', detail:'Stylized screen-height approximation using scene depth.'},
  {key:SCENE_EFFECT.CLUSTERED_LIGHTING, label:'Clustered lighting', detail:'Volumetric scattering from clustered point-light lists.'},
  {key:SCENE_EFFECT.ADAPTIVE_EXPOSURE, label:'Adaptive exposure', detail:'Temporal luminance adaptation before display tone mapping.'}
] as const;
export type EffectKey = typeof EFFECTS[number]['key'];
export type Quality = 'low' | 'medium' | 'high';

/** Disabled by default; temporal algorithms should be compared against this baseline. */
export function baselineConfig(): SceneEffectsOptions {
  return Object.fromEntries(EFFECTS.map(effect => [effect.key, false]));
}

/** Quality changes bounded sampling counts and effect resolution; it does not enable effects. */
export function qualityConfig(quality: Quality): SceneEffectsOptions {
  const resolutionScale = quality === 'high' ? 1 : 0.5;
  return {
    depthAwareBlur: {radius:quality === 'low' ? 2 : 4},
    ssao: {radius:4, intensity:1.5, resolutionScale},
    gtao: {radius:2.2, intensity:3.2, resolutionScale},
    ssgi: {resolutionScale, rayCount:quality === 'low' ? 3 : quality === 'medium' ? 7 : 12,
      stepCount:quality === 'high' ? 12 : 8},
    outlines: {color:[0.035,0.04,0.07,1], thickness:1.5},
    taa: {historyWeight:0.9, reprojection:SCENE_REPROJECTION.VELOCITY},
    motionBlur: {strength:1.5, sampleCount:quality === 'low' ? 6 : quality === 'medium' ? 10 : 16},
    ssr: {resolutionScale, sampleCount:quality === 'low' ? 16 : quality === 'medium' ? 48 : 80, maxRoughness:0.7},
    heightFog: {density:0.22, heightFalloff:3, fogColor:[0.18,0.34,0.48,0.6]},
    clusteredLighting: {resolutionScale, sampleCount:quality === 'low' ? 5 : quality === 'medium' ? 10 : 16},
    adaptiveExposure: {minimumExposure:0.125, maximumExposure:4, initialExposure:1}
  };
}
