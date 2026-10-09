// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';

export default defineConfig({
  root: fileURLToPath(new URL('./site', import.meta.url)),
  build: {outDir: '../dist', emptyOutDir: true},
  clearScreen: false
});
