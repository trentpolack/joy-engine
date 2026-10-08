// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import path from 'node:path';
import { findWorkspaceRoot, isEditorEnabled, readEditorPolicy } from './editor-policy.mjs';

/**
 * Guard direct editor builds and stale previews, and compile the same availability
 * into tool navigation. This never enables the development file-write service.
 * @param {string} slug Editor workspace directory name.
 * @param {string} [rootDirectory] Workspace containing joy.joyconfig; defaults to the
 * nearest one above the Vite root.
 * @returns {import('vite').Plugin}
 */
export function createEditorPolicyPlugin(slug, rootDirectory) {
  let enabled = false;
  return {
    name: 'joy-editor-production-policy',
    async config(config, environment) {
      const development = environment.command === 'serve' && !environment.isPreview;
      const workspaceRoot = rootDirectory ?? findWorkspaceRoot(path.resolve(config.root ?? process.cwd()));
      const policy = await readEditorPolicy(workspaceRoot, development);
      enabled = isEditorEnabled(slug, policy);
      if(environment.command === 'build' && !enabled) {
        throw new Error(`${slug} production build is disabled by joy.joyconfig. Enable its productionEnabled flag to build it.`);
      }
      return {
        define: {
          'import.meta.env.JOY_FORM_LAB_ENABLED': JSON.stringify(policy.formLab),
          'import.meta.env.JOY_PARTICLE_LAB_ENABLED': JSON.stringify(policy.particleLab)
        }
      };
    },
    configurePreviewServer(server) {
      if(!enabled) {
        server.middlewares.use((_request, response) => {
          response.statusCode = 404;
          response.end('Editor production preview is disabled by joy.joyconfig.');
        });
      }
    }
  };
}
