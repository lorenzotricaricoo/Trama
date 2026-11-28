import { invoke } from '@tauri-apps/api/core';
import type { Job } from '../jobs/useJobs';

/** A stretch of the transcript (see `src-tauri/src/meetings.rs`). */
export interface Segment {
  id: number;
  role: 'me' | 'others';
  startMs: number;
  endMs: number;
  text: string;
}

/** A meeting with its current transcript. */
export interface Meeting {
  id: string;
  /** Empty until the summary or the user gives one. */
  title: string;
  occurredAt: number;
  status: 'recording' | 'processing' | 'ready' | 'failed';
  durationMs: number | null;
  /** Why the meeting failed, as a code to translate. */
  errorCode: string | null;
  segments: Segment[];
  /** Language of the meeting (BCP 47), when known. */
  language: string | null;
  /** The tracks still on disk, for the player; empty once the audio is deleted. */
  audio: AudioTrack[];
  /** The audio was deleted by the retention setting. */
  audioDeleted: boolean;
}

/** An audio track of a meeting: an absolute path, played through the asset protocol. */
export interface AudioTrack {
  role: 'me' | 'others';
  path: string;
}

/** A key point of a summary, pinned to a moment when it has one. */
export interface KeyPoint {
  text: string;
  atMs: number | null;
}

/** The newest summary of a meeting (see `src-tauri/src/summary.rs`). */
export interface Summary {
  id: string;
  title: string;
  body: string;
  keyPoints: KeyPoint[];
  /** The model that wrote it; {@link USER_SUMMARY_MODEL} once the user edited it. */
  model: string;
}

/** {@link Summary.model} of a summary edited by the user (see `storage/model.rs`). */
export const USER_SUMMARY_MODEL = 'user';

export const getMeeting = (id: string) => invoke<Meeting>('get_meeting', { id });

export const sourceSummary = (sourceId: string) =>
  invoke<Summary | null>('source_summary', { sourceId });

/** Generates the summary again; progress arrives through the job events. */
export const regenerateSummary = (sourceId: string) =>
  invoke<Job>('regenerate_summary', { sourceId });

/** Stores the user's edit of the newest summary; the generated one stays in the history. */
export const editSummary = (sourceId: string, body: string, keyPoints: KeyPoint[]) =>
  invoke<Summary>('edit_summary', { sourceId, body, keyPoints });

/** Sets the title written by the user; an empty title gives it back to the summaries. */
export const setSourceTitle = (sourceId: string, title: string) =>
  invoke<{ sourceId: string; title: string; titleOrigin: 'auto' | 'user' }>('set_source_title', {
    sourceId,
    title,
  });

/** Transcribes a failed meeting again from its audio. */
export const retranscribe = (id: string) => invoke<Job>('retranscribe', { id });

/** Queues a failed job again. */
export const retryJob = (id: string) => invoke<Job>('retry_job', { id });
