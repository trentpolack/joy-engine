// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { fileURLToPath } from 'node:url';
import { defineConfig, searchForWorkspaceRoot } from 'vite';
import { createFaviconPlugin } from 'joy-engine/development/vite';
import { createEditorPolicyPlugin } from '../../tooling/editor-policy-plugin.mjs';

// Joy Editor ships its own copy of the Joy icon so it builds without the hosting workspace.
const EDITOR_ICON_PATH = fileURLToPath(new URL('../../assets/favicon.png', import.meta.url));

export default defineConfig({
  root: fileURLToPath(new URL('./site', import.meta.url)),
  server: {
    fs: {
      allow: [searchForWorkspaceRoot(process.cwd())]
    }
  },
  build: {outDir: '../dist', emptyOutDir: true},
  plugins: [createFaviconPlugin({iconPath: EDITOR_ICON_PATH}), createEditorPolicyPlugin('form-lab')]
});
