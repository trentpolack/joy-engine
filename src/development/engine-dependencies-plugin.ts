// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { Plugin, UserConfig } from 'vite';

/**
 * Engine runtime packages that Vite must pre-bundle when `joy-engine` is consumed as linked source.
 * The nested `joy-engine >` form resolves each package from the engine's own dependencies, so
 * consumers depend on the engine alone and never name its rendering or asset-loading backends.
 * Keep this list beside the engine's `dependencies` when a backend package is added or removed.
 */
const ENGINE_PREBUNDLED_DEPENDENCIES = Object.freeze([
  'joy-engine > @luma.gl/engine',
  'joy-engine > @luma.gl/shadertools',
  'joy-engine > @luma.gl/effects',
  'joy-engine > @loaders.gl/core',
  'joy-engine > @loaders.gl/gltf'
]);

/**
 * Add the engine's pre-bundled runtime dependencies to a consumer's Vite configuration.
 * Vite merges the returned `optimizeDeps.include` with the consumer's own list, so projects
 * keep their project-specific entries and lose only the engine-owned ones.
 * @returns A configuration-only plugin; it owns no server or build resources.
 */
export function createEngineDependenciesPlugin(): Plugin {
  return {
    name: 'joy-engine-dependencies',
    config(): UserConfig {
      return {optimizeDeps: {include: [...ENGINE_PREBUNDLED_DEPENDENCIES]}};
    }
  };
}
