// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import {fileURLToPath} from 'node:url';
import {defineConfig} from 'vite';
import {createFaviconPlugin} from 'joy-engine/development/vite';

export default defineConfig({
  root:fileURLToPath(new URL('./site',import.meta.url)),
  build:{outDir:'../dist',emptyOutDir:true},
  clearScreen:false,
  optimizeDeps:{include:['joy-engine > @luma.gl/engine','joy-engine > @luma.gl/shadertools','joy-engine > @luma.gl/effects']},
  plugins:[createFaviconPlugin({iconPath:fileURLToPath(new URL('../../assets/joyengine.png',import.meta.url))})]
});
