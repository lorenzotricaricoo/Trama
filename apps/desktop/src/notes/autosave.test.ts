import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Autosaver, type SaveStatus } from './autosave';
import { flushAll, onQuitFlush } from './flush';

const TIMING = { delay: 300, maxWait: 1500, retry: 3000 };

/** A save that resolves when the test says so. */
function deferredSave() {
  const calls: { text: string; resolve: () => void; reject: (error: Error) => void }[] = [];
  const save = vi.fn(
    (text: string) =>
      new Promise<void>((resolve, reject) => {
        calls.push({ text, resolve, reject });
      }),
  );
  return { save, calls };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('Autosaver', () => {
  it('saves the latest text after a pause in the typing', async () => {
    const save = vi.fn(() => Promise.resolve());
    const statuses: SaveStatus[] = [];
    const saver = new Autosaver(save, (s) => statuses.push(s), TIMING);
    saver.change('H');
    await vi.advanceTimersByTimeAsync(200);
    saver.change('He');
    await vi.advanceTimersByTimeAsync(200);
    expect(save).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(100);
    expect(save).toHaveBeenCalledExactlyOnceWith('He');
    expect(statuses.at(-1)).toBe('saved');
    expect(saver.dirty).toBe(false);
  });

  it('saves at least every maxWait while the typing goes on', async () => {
    const save = vi.fn(() => Promise.resolve());
    const saver = new Autosaver(save, undefined, TIMING);
    let text = '';
    for (let i = 0; i < 8; i++) {
      text += 'a';
      saver.change(text);
      await vi.advanceTimersByTimeAsync(250);
    }
    // 2 s of typing without a 300 ms pause: written once at 1.5 s.
    expect(save).toHaveBeenCalledExactlyOnceWith('aaaaaa');
  });

  it('flushes at once and resolves when the text is written', async () => {
    const { save, calls } = deferredSave();
    const saver = new Autosaver(save, undefined, TIMING);
    saver.change('# Draft');
    const flushed = saver.flush();
    expect(save).toHaveBeenCalledWith('# Draft');
    let done = false;
    void flushed.then(() => (done = true));
    await vi.advanceTimersByTimeAsync(0);
    expect(done).toBe(false);
    calls[0]?.resolve();
    expect(await flushed).toBe(true);
    expect(saver.dirty).toBe(false);
    // Nothing left for the timer.
    await vi.advanceTimersByTimeAsync(TIMING.maxWait);
    expect(save).toHaveBeenCalledOnce();
  });

  it('writes one save at a time and only the newest text', async () => {
    const { save, calls } = deferredSave();
    const saver = new Autosaver(save, undefined, TIMING);
    saver.change('one');
    void saver.flush();
    saver.change('two');
    saver.change('three');
    const flushed = saver.flush();
    await vi.advanceTimersByTimeAsync(0);
    expect(save).toHaveBeenCalledOnce();
    calls[0]?.resolve();
    await vi.advanceTimersByTimeAsync(0);
    expect(save).toHaveBeenLastCalledWith('three');
    calls[1]?.resolve();
    await flushed;
    expect(save.mock.calls.map(([text]) => text)).toEqual(['one', 'three']);
  });

  it('keeps the text after a failed save and tries again', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    const { save, calls } = deferredSave();
    const statuses: SaveStatus[] = [];
    const saver = new Autosaver(save, (s) => statuses.push(s), TIMING);
    saver.change('keep me');
    const flushed = saver.flush();
    calls[0]?.reject(new Error('disk full'));
    expect(await flushed).toBe(false);
    expect(statuses.at(-1)).toBe('error');
    expect(saver.dirty).toBe(true);
    let clean = false;
    void saver.whenClean().then(() => (clean = true));
    await vi.advanceTimersByTimeAsync(TIMING.retry);
    expect(save).toHaveBeenLastCalledWith('keep me');
    expect(clean).toBe(false);
    calls[1]?.resolve();
    await vi.advanceTimersByTimeAsync(0);
    expect(statuses.at(-1)).toBe('saved');
    expect(clean).toBe(true);
    expect(error).toHaveBeenCalled();
    error.mockRestore();
  });
});

describe('flushAll', () => {
  it('runs every registered write and says whether all of them saved', async () => {
    const saved = vi.fn(() => Promise.resolve(true));
    const unsaved = vi.fn(() => Promise.resolve(false));
    const broken = vi.fn(() => Promise.reject(new Error('no')));
    const removeSaved = onQuitFlush(saved);
    expect(await flushAll()).toBe(true);
    const removeUnsaved = onQuitFlush(unsaved);
    expect(await flushAll()).toBe(false);
    removeUnsaved();
    const removeBroken = onQuitFlush(broken);
    expect(await flushAll()).toBe(false);
    removeBroken();
    removeSaved();
    expect(await flushAll()).toBe(true);
    expect(saved).toHaveBeenCalledTimes(3);
    expect(unsaved).toHaveBeenCalledOnce();
    expect(broken).toHaveBeenCalledOnce();
  });
});
