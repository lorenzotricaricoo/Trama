import { renderHook } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { OPEN_MEETING_EVENT, RECORDING_ERROR_EVENT, useShellEvents } from './useShellEvents';

const tauri = vi.hoisted(() => ({
  handlers: new Map<string, (event: { payload: unknown }) => void>(),
}));

vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => true }));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((name: string, handler: (event: { payload: unknown }) => void) => {
    tauri.handlers.set(name, handler);
    return Promise.resolve(() => tauri.handlers.delete(name));
  }),
}));

beforeEach(() => {
  tauri.handlers.clear();
});

function emit(name: string, payload: unknown) {
  const handler = tauri.handlers.get(name);
  if (!handler) throw new Error(`nobody listens to ${name}`);
  handler({ payload });
}

describe('useShellEvents', () => {
  it('reports a recording error from the menu bar item or the shortcut', () => {
    const onRecordingError = vi.fn();
    renderHook(() => useShellEvents({ onRecordingError, onOpenMeeting: vi.fn() }));
    emit(RECORDING_ERROR_EVENT, 'microphone_denied');
    expect(onRecordingError).toHaveBeenCalledExactlyOnceWith('microphone_denied');
    // Anything but a code is unexpected.
    emit(RECORDING_ERROR_EVENT, { odd: true });
    expect(onRecordingError).toHaveBeenLastCalledWith('internal');
  });

  it('opens a meeting chosen in the menu bar item with the latest handler', () => {
    const first = vi.fn();
    const latest = vi.fn();
    const { rerender } = renderHook(
      ({ onOpenMeeting }) => useShellEvents({ onRecordingError: vi.fn(), onOpenMeeting }),
      { initialProps: { onOpenMeeting: first } },
    );
    rerender({ onOpenMeeting: latest });
    emit(OPEN_MEETING_EVENT, 'meeting-1');
    expect(latest).toHaveBeenCalledExactlyOnceWith('meeting-1');
    expect(first).not.toHaveBeenCalled();
    emit(OPEN_MEETING_EVENT, 42);
    expect(latest).toHaveBeenCalledOnce();
  });

  it('stops listening when unmounted', async () => {
    const { unmount } = renderHook(() =>
      useShellEvents({ onRecordingError: vi.fn(), onOpenMeeting: vi.fn() }),
    );
    expect(tauri.handlers.size).toBe(2);
    unmount();
    await vi.waitFor(() => expect(tauri.handlers.size).toBe(0));
  });
});
