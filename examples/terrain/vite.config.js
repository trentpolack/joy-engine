import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
export default defineConfig({
  root:fileURLToPath(new URL('./site',import.meta.url)),
  build:{outDir:'../dist',emptyOutDir:true},clearScreen:false
});
