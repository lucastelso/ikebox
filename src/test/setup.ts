import '@testing-library/jest-dom/vitest';
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup } from '@testing-library/react';
import { afterEach } from 'vitest';

// Cada teste começa com DOM e IPC limpos.
afterEach(() => {
  cleanup();
  clearMocks();
});

// O jsdom não implementa ResizeObserver (usado por componentes Radix).
class ResizeObserverStub {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}
globalThis.ResizeObserver ??= ResizeObserverStub as unknown as typeof ResizeObserver;
