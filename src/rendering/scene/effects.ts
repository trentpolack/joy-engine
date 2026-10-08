// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Keep projects on the Joy Engine boundary while exposing luma.gl's maintained,
// portable shader passes. These are shader descriptions/pipelines; the project
// remains responsible for choosing inputs and the owner remains responsible for
// destroying the ShaderPassRenderer that executes them.
export {
  bloom,
  bloomShaderPassPipeline,
  createBloomShaderPassPipeline,
  brightnessContrast,
  denoise,
  gaussianBlur,
  hueSaturation,
  noise,
  sepia,
  tiltShift,
  toneMapping,
  vibrance,
  vignette,
  fxaa,
  createMotionBlurShaderPassPipeline,
  createOutlineShaderPassPipeline,
  createSSAOShaderPassPipeline,
  createTAAShaderPassPipeline
} from '@luma.gl/effects';
