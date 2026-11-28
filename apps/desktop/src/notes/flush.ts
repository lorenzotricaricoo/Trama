import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useEffect, useRef } from 'react';

/** A write to finish before quitting; resolves to whether everything was saved. */
export type QuitFlush = () => Promise<boolean>;

/**
 * Writes that must finish before the app quits: open editors, and closed ones whose last
 * save failed and is being tried again.
 */
const pending = new Set<QuitFlush>();

/** Registers a write to finish before quitting; returns the function that removes it. */
export function onQuitFlush(flush: QuitFlush): () => void {
  pending.add(flush);
  return () => {
    pending.delete(flush);
  };
}

/** Runs every registered write. Resolves to true only if every one saved. */
export async function flushAll(): Promise<boolean> {
  const results = await Promise.allSettled([...pending].map((flush) => flush()));
  return results.every((result) => result.status === 'fulfilled' && result.value);
}

/** Event the shell sends when the user quits or closes the window (`src-tauri/src/quit.rs`). */
export const QUIT_REQUESTED_EVENT = 'quit-requested';

/** After a quit kept the app open, quitting again within this time quits anyway, in ms. */
export const QUIT_AGAIN_MS = 30_000;

/**
 * Lets the shell quit only after the open editors have saved.
 *
 * If a save fails, the quit is cancelled: the text stays on screen with the error, and
 * `onKept` tells the user. Quitting again within {@link QUIT_AGAIN_MS} quits anyway, so a
 * broken disk never traps the user. A frontend that does not answer at all is handled by
 * the shell's own timeout.
 */
export function useFlushBeforeQuit(onKept: () => void) {
  const latest = useRef(onKept);
  useEffect(() => {
    latest.current = onKept;
  });

  useEffect(() => {
    if (!isTauri()) return;
    let keptAt: number | undefined;
    const unlisten = listen(QUIT_REQUESTED_EVENT, () => {
      void flushAll()
        .then((saved) => {
          const insisted = keptAt !== undefined && Date.now() - keptAt < QUIT_AGAIN_MS;
          if (saved || insisted) return invoke('quit_ready');
          keptAt = Date.now();
          latest.current();
          return invoke('quit_cancelled');
        })
        .catch((error: unknown) => {
          // The shell quits on its own after a short wait.
          console.error('Cannot answer the request to quit', error);
        });
    });
    return () => {
      void unlisten.then((stop) => {
        stop();
      });
    };
  }, []);
}
