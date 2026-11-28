import type { DragDropEvent } from '@tauri-apps/api/webview';

type Handler = (event: { payload: DragDropEvent }) => void;

/**
 * The window's drag and drop in tests: `test/setup.ts` replaces the Tauri webview with this,
 * and a test calls `drag` to hold files over the window or drop them.
 */
export const webview = {
  handler: undefined as Handler | undefined,
  drag(payload: DragDropEvent) {
    this.handler?.({ payload });
  },
};
