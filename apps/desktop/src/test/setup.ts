import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach, vi } from 'vitest';

// Trama is macOS-first: tests run as on a Mac (⌘ shortcuts) unless they say otherwise.
const MAC_PLATFORM = 'MacIntel';
Object.defineProperty(navigator, 'platform', { configurable: true, value: MAC_PLATFORM });

// The window's drag and drop: tests drive it through `webview.drag`.
vi.mock('@tauri-apps/api/webview', async () => {
  const { webview } = await import('./webview');
  return {
    getCurrentWebview: () => ({
      onDragDropEvent: (handler: NonNullable<typeof webview.handler>) => {
        webview.handler = handler;
        return Promise.resolve(() => {
          if (webview.handler === handler) webview.handler = undefined;
        });
      },
    }),
  };
});

afterEach(() => {
  cleanup();
  document.documentElement.className = '';
  Object.defineProperty(navigator, 'platform', { configurable: true, value: MAC_PLATFORM });
});

// jsdom has no matchMedia.
Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: vi.fn((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
});
