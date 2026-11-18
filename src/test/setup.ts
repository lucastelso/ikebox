import '@testing-library/jest-dom/vitest';
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup } from '@testing-library/react';
import { afterEach } from 'vitest';

// Each test starts with a clean DOM and IPC.
afterEach(() => {
  cleanup();
  clearMocks();
});

// jsdom does not implement ResizeObserver (used by Radix components).
class ResizeObserverStub {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}
globalThis.ResizeObserver ??= ResizeObserverStub as unknown as typeof ResizeObserver;

// Radix's Select uses pointer capture and scrollIntoView, which jsdom does not
// implement.
if (!Element.prototype.hasPointerCapture) {
  Element.prototype.hasPointerCapture = () => false;
  Element.prototype.releasePointerCapture = () => {};
}
Element.prototype.scrollIntoView ??= () => {};
