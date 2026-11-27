import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useEffect, useState } from 'react';

/** A background job, as sent by the core (see `src-tauri/src/jobs.rs`). */
export interface Job {
  id: string;
  kind: 'transcribe' | 'summarize' | 'delete_audio';
  targetId: string;
  state: 'queued' | 'running' | 'done' | 'failed' | 'cancelled';
  attempts: number;
  maxAttempts: number;
  /** Error code, translated by the interface. */
  errorCode: string | null;
  /** Progress of a running job, from 0 to 1, when known. */
  progress: number | null;
}

/** Event the core sends for every job change. */
export const JOB_EVENT = 'job';

/** Jobs kept in the list: active ones, and failed ones until retried. */
function isShown(job: Job): boolean {
  return job.state === 'queued' || job.state === 'running' || job.state === 'failed';
}

/** Applies a job change to the list, keeping the last known progress of a running job. */
export function applyJob(jobs: readonly Job[], change: Job): Job[] {
  const previous = jobs.find((job) => job.id === change.id);
  const job: Job =
    change.state === 'running' && change.progress === null && previous
      ? { ...change, progress: previous.progress }
      : change;
  const others = jobs.filter((existing) => existing.id !== change.id);
  return isShown(job) ? [...others, job] : others;
}

/**
 * Background jobs and their progress, kept up to date by the core's events. Outside the
 * app (a plain browser, tests without Tauri) the list stays empty.
 */
export function useJobs(): Job[] {
  const [jobs, setJobs] = useState<Job[]>([]);

  useEffect(() => {
    if (!isTauri()) return;
    let active = true;
    const updated = new Set<string>();
    const unlisten = listen<Job>(JOB_EVENT, (event) => {
      updated.add(event.payload.id);
      setJobs((current) => applyJob(current, event.payload));
    });
    // Listen first, then load: an event that arrives meanwhile is newer than the list.
    void unlisten
      .then(() => invoke<Job[]>('active_jobs'))
      .then((initial) => {
        if (!active) return;
        setJobs((current) =>
          initial
            .filter((job) => !updated.has(job.id))
            .reduce((list, job) => applyJob(list, job), current),
        );
      })
      .catch((error: unknown) => {
        console.error('Cannot load the background jobs', error);
      });
    return () => {
      active = false;
      void unlisten.then((stop) => {
        stop();
      });
    };
  }, []);

  return jobs;
}
