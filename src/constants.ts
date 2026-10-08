// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Data-only public entry point: safe for workers, editors and game runtime imports.

// Rendering constants.
export {
  GPU_BACKEND,
  GPU_TEXTURE_FORMAT,
  GPU_FILTER,
  GPU_ADDRESS_MODE,
  GPU_TOPOLOGY,
  GPU_VERTEX_FORMAT,
  GPU_COMPARE,
  SHADER_ENTRY_POINT
} from './rendering/gpu/constants.ts';

// Rendering (lighting/material) constants.
export {
  MATERIAL_ALPHA_MODE
} from './rendering/pbr/constants.ts';

// Rendering (geometry) constants.
export {
  PRIMITIVE_SHAPE
} from './rendering/geometry/constants.ts';

// Renderer (postprocessing) constants.
export {
  TONE_MAPPER,
  POST_PROCESSING_KEY,
  BLOOM_QUALITY,
  AGX_LOOK,
  FILM_GRAIN_NOISE
} from './rendering/postprocessor/constants.ts';

// Renderer (scene) constants.
export {
  SCENE_EFFECT,
  SCENE_NORMAL_SOURCE,
  SCENE_REPROJECTION,
  SCENE_EFFECT_PROPERTY
} from './rendering/scene/constants.ts';

// Gameplay constants.
export {
  ENTITY_TYPE,
  TRANSFORM_PROPERTY
} from './gameplay/constants.ts';

// Editor preview constants.
export {
  PREVIEW_CHANNEL,
  PREVIEW_COMMAND,
  PREVIEW_STATE
} from './runtime/preview-constants.ts';

// Input and Key-Binding Constants.
export {
  KEY_CODE,
  INPUT_CONTEXT,
  INPUT_BINDING_TYPE
} from './browser/input/constants.ts';

// Browser constants.
export {
  BROWSER_EVENT
} from './browser/constants.ts';

// In-application tuning constants.
export {
  TUNING_APPLY_MODE
} from './browser/tuning/constants.ts';

// Particle system constants.
export {
  PARTICLE_RENDERER,
  PARTICLE_PARAMETER_TYPE,
  PARTICLE_VALUE_KIND
} from './particles/constants.ts';

// FORM scripting and configuration constants.
export {
  FORM_NODE_KIND,
  FORM_KEYWORD,
  FORM_VALUE_KIND,
  FORM_ATTRIBUTE_DOMAIN,
  FORM_PARAMETER_TYPE,
  FORM_COMMAND,
  FORM_MATERIAL,
  FORM_ATTRIBUTE
} from './form/constants.ts';
