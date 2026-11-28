import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useEffect } from 'react';

/** Writes that must finish before the app quits (editors with unsaved text). */
const pending = new Set<() => Promise<void>>();

/** Registers a write to finish before quitting; returns the function that removes it. */
export function onQuitFlush(flush: () => Promise<void>): () => void {
  pending.add(flush);
  return () => {
    pending.delete(flush);
  };
}

/** Runs every registered write; a failed one does not stop the others. */
export async function flushAll(): Promise<void> {
  await Promise.allSettled([...pending].map((flush) => flush()));
}

/** Event the shell sends when the user quits or closes the window (`src-tauri/src/quit.rs`). */
export const QUIT_REQUESTED_EVENT = 'quit-requested';

/**
 * Lets the shell quit only after the open editors have saved. The shell waits a short
 * time at most, so a stuck write never keeps the app open.
 */
export function useFlushBeforeQuit() {
  useEffect(() => {
    if (!isTauri()) return;
    const unlisten = listen(QUIT_REQUESTED_EVENT, () => {
      void flushAll()
        .then(() => invoke('quit_ready'))
        .catch((error: unknown) => {
          // The shell quits on its own after a short wait.
          console.error('Cannot tell the shell to quit', error);
        });
    });
    return () => {
      void unlisten.then((stop) => {
        stop();
      });
    };
  }, []);
}
