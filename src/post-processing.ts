// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Shared by browser runtime configuration and Node build tools. Keep this entry free of DOM, CSS, and shader asset imports, including transitive dependencies.
export type { PostProcessingConfig } from './rendering/postprocessor/config.ts';
export type { PostProcessingKey } from './rendering/postprocessor/config.ts';
export type { PostProcessingControl } from './rendering/postprocessor/config.ts';
export type { PostProcessingProfile } from './rendering/postprocessor/profile.ts';

export {
  POST_PROCESSING_SCHEMA,
  DEFAULT_POST_PROCESSING,
  normalizePostProcessingConfig,
  createPostProcessingUniforms,
  toneMapperIndex,
  addPostProcessingShaders,
} from './rendering/postprocessor/config.ts';
export {
  POST_PROCESSING_PRESETS,
  validatePostProcessingProfile,
  resolvePostProcessingProfile,
} from './rendering/postprocessor/profile.ts';
