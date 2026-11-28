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
}

export const getMeeting = (id: string) => invoke<Meeting>('get_meeting', { id });

export const sourceSummary = (sourceId: string) =>
  invoke<Summary | null>('source_summary', { sourceId });

/** Generates the summary again; progress arrives through the job events. */
export const regenerateSummary = (sourceId: string) =>
  invoke<Job>('regenerate_summary', { sourceId });

/** Queues a failed job again. */
export const retryJob = (id: string) => invoke<Job>('retry_job', { id });
