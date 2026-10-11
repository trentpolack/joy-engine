// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

export interface PreviewLightingSettings {
  directionX: number;
  directionY: number;
  directionZ: number;
  colorR: number;
  colorG: number;
  colorB: number;
  intensity: number;
  environmentIntensity: number;
  environmentRotation: number;
  castsShadow: boolean;
  quality: 'low' | 'balanced' | 'cinematic';
}

export const DEFAULT_PREVIEW_LIGHTING: Readonly<PreviewLightingSettings> = Object.freeze({
  directionX: 0.35, directionY: 0.85, directionZ: 0.4,
  colorR: 1, colorG: 1, colorB: 1, intensity: 3,
  environmentIntensity: 1, environmentRotation: 0, castsShadow: true, quality: 'low'
});

export const PREVIEW_LIGHTING_FIELDS = [
  {key: 'directionX', title: 'Direction X', min: -1, max: 1, step: 0.05},
  {key: 'directionY', title: 'Direction Y', min: -1, max: 1, step: 0.05},
  {key: 'directionZ', title: 'Direction Z', min: -1, max: 1, step: 0.05},
  {key: 'colorR', title: 'Light red', min: 0, max: 4, step: 0.05},
  {key: 'colorG', title: 'Light green', min: 0, max: 4, step: 0.05},
  {key: 'colorB', title: 'Light blue', min: 0, max: 4, step: 0.05},
  {key: 'intensity', title: 'Direct intensity', min: 0, max: 12, step: 0.1},
  {key: 'environmentIntensity', title: 'Environment intensity', min: 0, max: 4, step: 0.1},
  {key: 'environmentRotation', title: 'Environment rotation (rad)', min: 0, max: Math.PI*2, step: 0.05}
] as const;

/** Original studio panels and floor/ceiling illumination; no external image assets. */
export const STUDIO_ENVIRONMENT = Object.freeze({
  width: 256, height: 128, floor: 0.045, ceiling: 0.22,
  panelPower: 7, panelWidth: 0.08, panelHeight: 0.55,
  panels: [0.12, 0.58], panelColors: [[1, 0.89, 0.74], [0.65, 0.8, 1]],
  specularSize: 64, irradianceSize: 16, brdfSize: 128, sampleCount: 64
});

/** Generate linear HDR equirectangular pixels for the engine's environment adapter. */
export function createStudioPixels() {
  const config = STUDIO_ENVIRONMENT;
  const data = new Float32Array(config.width*config.height*4);
  for(let y = 0; y < config.height; y++) {
    const latitude = (y + 0.5)/config.height;
    const ambient = config.floor + (config.ceiling - config.floor)*(1 - latitude);
    for(let x = 0; x < config.width; x++) {
      const longitude = (x + 0.5)/config.width;
      const offset = (y*config.width + x)*4;
      for(let channel = 0; channel < 3; channel++) {
        let radiance = ambient;
        for(let panel = 0; panel < config.panels.length; panel++) {
          const distance = Math.min(Math.abs(longitude - config.panels[panel]), 1 - Math.abs(longitude - config.panels[panel]));
          const vertical = (latitude - 0.38)/config.panelHeight;
          const shape = Math.exp(-((distance/config.panelWidth)**8) - vertical**8);
          radiance+= config.panelPower*config.panelColors[panel][channel]*shape;
        }
        data[offset + channel] = radiance;
      }
      data[offset + 3] = 1;
    }
  }
  return {width: config.width, height: config.height, data};
}

/** Validate before changing the accepted viewport lighting. */
export function validatePreviewLighting(settings: PreviewLightingSettings) {
  for(const field of PREVIEW_LIGHTING_FIELDS) {
    const value = settings[field.key];
    if(!Number.isFinite(value) || value < field.min || value > field.max) {
      throw new RangeError(`${field.title} must be between ${field.min} and ${field.max}.`);
    }
  }
  if(Math.hypot(settings.directionX, settings.directionY, settings.directionZ) < 0.0001) {
    throw new RangeError('Light direction must be nonzero.');
  }
  if(!['low', 'balanced', 'cinematic'].includes(settings.quality) || typeof settings.castsShadow !== 'boolean') {
    throw new RangeError('Preview shadow settings are invalid.');
  }
}
