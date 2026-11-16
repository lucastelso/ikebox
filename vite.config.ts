/// <reference types="vitest/config" />
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// Configuração para o Tauri: porta fixa, sem limpar a tela (logs do Rust),
// e o diretório src-tauri fora do watcher.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: '127.0.0.1',
    watch: { ignored: ['**/src-tauri/**', '**/target/**'] },
    // A raiz do servidor de dev é a pasta do projeto: arquivos privados que
    // alguém deixe nela (perfis exportados, swanctl.conf com PSK) não podem
    // ser servidos. Os quatro primeiros são o padrão do Vite.
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
