import { isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useEffect, useRef } from 'react';

/** Sent when starting or stopping from the menu bar item or the global shortcut failed. */
export const RECORDING_ERROR_EVENT = 'recording-error';

/** Sent when a meeting is chosen in the menu bar item. */
export const OPEN_MEETING_EVENT = 'open-meeting';

export interface ShellEventHandlers {
  /** The error code of a recording started or stopped outside the window. */
  onRecordingError: (code: string) => void;
  onOpenMeeting: (meetingId: string) => void;
}

/**
 * Runs what the shell asks of the main window from outside it: the menu bar item and the
 * global shortcut (see `src-tauri/src/tray.rs` and `recording::toggle`).
 */
export function useShellEvents(handlers: ShellEventHandlers) {
  const latest = useRef(handlers);
  useEffect(() => {
    latest.current = handlers;
  });

  useEffect(() => {
    if (!isTauri()) return;
    const unlisten = [
      listen<unknown>(RECORDING_ERROR_EVENT, (event) => {
        latest.current.onRecordingError(
          typeof event.payload === 'string' ? event.payload : 'internal',
        );
      }),
      listen<unknown>(OPEN_MEETING_EVENT, (event) => {
        if (typeof event.payload === 'string') latest.current.onOpenMeeting(event.payload);
      }),
    ];
    return () => {
      for (const stop of unlisten) void stop.then((off) => off());
    };
  }, []);
}
