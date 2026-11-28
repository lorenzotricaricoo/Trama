import { isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useEffect, useRef, useState } from 'react';
import {
  RECORDING_STATE_EVENT,
  recordingStatus,
  type RecordingState,
  type RecordingStatus,
} from './api';

/** A status with the moment it arrived, to keep the timer going between updates. */
interface Received {
  status: RecordingStatus;
  at: number;
}

/**
 * The running recording, kept up to date by the core's events. `onStopped` runs when a
 * recording stops, from any window.
 */
export function useRecordingState(onStopped?: (meetingId: string) => void) {
  const [received, setReceived] = useState<Received | null>(null);
  const stopped = useRef(onStopped);
  useEffect(() => {
    stopped.current = onStopped;
  });

  useEffect(() => {
    if (!isTauri()) return;
    let active = true;
    let fromEvent = false;
    const unlisten = listen<RecordingState>(RECORDING_STATE_EVENT, (event) => {
      fromEvent = true;
      const { recording, stoppedMeetingId } = event.payload;
      setReceived(recording ? { status: recording, at: performance.now() } : null);
      if (stoppedMeetingId) stopped.current?.(stoppedMeetingId);
    });
    // Listen first, then ask: an event that arrives meanwhile is newer.
    void unlisten
      .then(() => recordingStatus())
      .then((status) => {
        if (active && !fromEvent) setReceived(status ? { status, at: performance.now() } : null);
      })
      .catch((error: unknown) => {
        console.error('Cannot read the recording status', error);
      });
    return () => {
      active = false;
      void unlisten.then((stop) => {
        stop();
      });
    };
  }, []);

  /** Sets the status returned by a command, before its event arrives. */
  const set = (status: RecordingStatus | null) =>
    setReceived(status ? { status, at: performance.now() } : null);

  return { status: received?.status ?? null, receivedAt: received?.at ?? 0, set };
}

/** Time recorded, updated a few times a second while recording. */
export function useElapsed(status: RecordingStatus | null, receivedAt: number): number {
  const [now, setNow] = useState(() => performance.now());
  useEffect(() => {
    if (!status) return;
    const timer = window.setInterval(() => setNow(performance.now()), 250);
    return () => window.clearInterval(timer);
  }, [status]);
  return status ? status.elapsedMs + Math.max(0, now - receivedAt) : 0;
}
