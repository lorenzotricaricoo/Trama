import { invoke } from '@tauri-apps/api/core';

/** A running recording (see `src-tauri/src/recording.rs`). */
export interface RecordingStatus {
  meetingId: string;
  startedAt: number;
  /** Time recorded when the status was sent. */
  elapsedMs: number;
}

/** Sent to every window when a recording starts or stops. */
export interface RecordingState {
  recording: RecordingStatus | null;
  /** The meeting that was just stopped. */
  stoppedMeetingId: string | null;
}

/** Which side of the meeting a track records: the user or the others. */
export type TrackSource = 'microphone' | 'system';

/** Live level of a track, about 20 times a second. */
export interface TrackLevel {
  source: TrackSource;
  peakDb: number;
  rmsDb: number;
}

/** A warning or a device change during the recording. */
export interface RecordingNotice {
  source: TrackSource;
  kind: 'started' | 'device_changed' | 'interrupted' | 'problem';
  /** Capture error code for `interrupted` and `problem`. */
  code: string | null;
}

/** A note taken while recording, pinned to its moment. */
export interface RecordingNote {
  id: string;
  meetingId: string;
  atMs: number;
  text: string;
}

export const RECORDING_STATE_EVENT = 'recording-state';
export const RECORDING_LEVEL_EVENT = 'recording-level';
export const RECORDING_NOTICE_EVENT = 'recording-notice';

/** Starts recording a new meeting; rejects with an error code. */
export const startRecording = () => invoke<RecordingStatus>('start_recording');

/** Stops the recording and queues its transcription; resolves to the meeting id. */
export const stopRecording = () => invoke<string>('stop_recording');

/** The running recording, if any. */
export const recordingStatus = () => invoke<RecordingStatus | null>('recording_status');

/** Shows the recording panel again after it was closed. */
export const showRecordingPanel = () => invoke<void>('show_recording_panel');

/** Shrinks the panel to a pill, or brings it back. */
export const setPanelCompact = (compact: boolean) =>
  invoke<void>('set_recording_panel_compact', { compact });

/** Adds a note at the current moment of the recording. */
export const addRecordingNote = (text: string) =>
  invoke<RecordingNote>('add_recording_note', { text });

/** The notes of a meeting, in order of time. */
export const recordingNotes = (meetingId: string) =>
  invoke<RecordingNote[]>('recording_notes', { meetingId });
