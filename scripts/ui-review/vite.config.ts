// UI review build (phase 5 of the post-0.2.0 program): the real app with the
// Tauri IPC replaced by fixtures. Never part of the app build.
import { fileURLToPath } from 'node:url';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

const repo = fileURLToPath(new URL('../..', import.meta.url));

export default defineConfig({
  root: fileURLToPath(new URL('.', import.meta.url)),
  publicDir: `${repo}/public`,
  plugins: [react(), tailwindcss()],
  resolve: { alias: { '@': `${repo}/src` } },
  build: { outDir: `${repo}/target/ui-review/site`, emptyOutDir: true, target: 'es2022' },
});
