/// <reference types="vitest/config" />
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// Configuration for Tauri: fixed port, no clearing the screen (Rust logs),
// and the src-tauri directory out of the watcher.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: '127.0.0.1',
    watch: { ignored: ['**/src-tauri/**', '**/target/**'] },
    // The dev server's root is the project folder: private files someone leaves
    // in it (exported profiles, a swanctl.conf with a PSK) must not be served.
    // The first four are Vite's default.
    fs: {
      deny: [
        '.env',
        '.env.*',
        '*.{crt,pem}',
        '**/.git/**',
        '*.{conf,key,p12}',
        '**/trabalho*.json',
        '**/presets.local/**',
        '**/vpn-*/**',
        '**/target/**',
      ],
    },
  },
  build: {
    target: 'es2022',
    sourcemap: false,
  },
  resolve: {
    alias: { '@': '/src' },
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts'],
    include: ['src/**/*.test.{ts,tsx}'],
    restoreMocks: true,
  },
});
