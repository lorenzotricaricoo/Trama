/**
 * Writes still running for note screens that were left, by navigation entry: the last save
 * and, for a new note, its creation. Each resolves to the id of the note it wrote (none if
 * nothing was typed or the creation failed).
 *
 * A screen that opens waits for them before reading a note, so going Back to a note whose
 * last save (or creation) is still running shows the saved text and never creates the note
 * a second time.
 */
const writes = new Map<number, Promise<string | undefined>>();

/** Longest wait for those writes: a stuck write must not keep notes from opening, in ms. */
export const LEFT_SCREEN_WAIT_MS = 3000;

/** Records the writes still running for the screen of `entry`, which was just left. */
export function trackLeftScreen(entry: number, done: Promise<string | undefined>) {
  writes.set(entry, done);
  void done.finally(() => {
    if (writes.get(entry) === done) writes.delete(entry);
  });
}

/**
 * Waits for the writes of every left screen, at most {@link LEFT_SCREEN_WAIT_MS}; resolves
 * to the id of the note written by the screen of `entry`, if known by then.
 */
export async function afterLeftScreens(entry: number): Promise<string | undefined> {
  const own = writes.get(entry);
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<'timeout'>((resolve) => {
    timer = setTimeout(() => resolve('timeout'), LEFT_SCREEN_WAIT_MS);
  });
  const all = Promise.allSettled(writes.values()).then(() => own);
  try {
    const result = await Promise.race([all, timeout]);
    return result === 'timeout' ? undefined : result;
  } finally {
    clearTimeout(timer);
  }
}
