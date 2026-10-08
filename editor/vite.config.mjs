// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import { createFaviconPlugin } from 'joy-engine/development/vite';
import { createEditorPolicyPlugin } from './tooling/editor-policy-plugin.mjs';

// Joy Editor ships its own copy of the Joy icon so it builds without the hosting workspace.
const EDITOR_ICON_PATH = fileURLToPath(new URL('./assets/favicon.png', import.meta.url));

export default defineConfig({
  root: fileURLToPath(new URL('./site', import.meta.url)),
  base: '/editor/',
  plugins: [createFaviconPlugin({iconPath: EDITOR_ICON_PATH}), createEditorPolicyPlugin('joy-editor')],
  build: {
    outDir: '../dist',
    emptyOutDir: true,
    rollupOptions: {
      input: {
        preview: fileURLToPath(new URL('./site/preview.html', import.meta.url)),
        workspace: fileURLToPath(new URL('./site/index.html', import.meta.url)),
        level: fileURLToPath(new URL('./site/level.html', import.meta.url))
      }
    }
  }
});
