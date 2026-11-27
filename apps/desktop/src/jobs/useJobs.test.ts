import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { applyJob, JOB_EVENT, useJobs, type Job } from './useJobs';

type Handler = (event: { payload: Job }) => void;

const tauri = vi.hoisted(() => ({
  handlers: new Map<string, (event: { payload: unknown }) => void>(),
  invoke: vi.fn(),
  isTauri: vi.fn(() => true),
  unlisten: vi.fn(),
}));

vi.mock('@tauri-apps/api/core', () => ({ invoke: tauri.invoke, isTauri: tauri.isTauri }));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((name: string, handler: Handler) => {
    tauri.handlers.set(name, handler as (event: { payload: unknown }) => void);
    return Promise.resolve(tauri.unlisten);
  }),
}));

function job(overrides: Partial<Job> = {}): Job {
  return {
    id: 'job-1',
    kind: 'transcribe',
    targetId: 'meeting-1',
    state: 'queued',
    attempts: 0,
    maxAttempts: 5,
    errorCode: null,
    progress: null,
    ...overrides,
  };
}

function emit(payload: Job) {
  act(() => {
    tauri.handlers.get(JOB_EVENT)?.({ payload });
  });
}

beforeEach(() => {
  tauri.handlers.clear();
  tauri.invoke.mockReset();
  tauri.isTauri.mockReturnValue(true);
  tauri.unlisten.mockReset();
});

describe('applyJob', () => {
  it('keeps the progress of a running job across state-only updates', () => {
    const running = applyJob([], job({ state: 'running', progress: 0.62 }));
    expect(applyJob(running, job({ state: 'running', attempts: 1 }))[0]?.progress).toBe(0.62);
  });

  it('drops finished jobs but keeps failed ones', () => {
    const list = [job()];
    expect(applyJob(list, job({ state: 'done' }))).toEqual([]);
    expect(applyJob(list, job({ state: 'cancelled' }))).toEqual([]);
    expect(applyJob(list, job({ state: 'failed', errorCode: 'model_missing' }))).toHaveLength(1);
  });
});

describe('useJobs', () => {
  it('loads active jobs, then follows progress events', async () => {
    tauri.invoke.mockResolvedValue([job()]);
    const { result, unmount } = renderHook(() => useJobs());
    await waitFor(() => {
      expect(result.current).toHaveLength(1);
    });
    expect(tauri.invoke).toHaveBeenCalledWith('active_jobs');

    emit(job({ state: 'running', attempts: 1, progress: 0 }));
    emit(job({ state: 'running', attempts: 1, progress: 0.62 }));
    expect(result.current[0]).toMatchObject({ state: 'running', progress: 0.62 });

    emit(job({ state: 'done', attempts: 1 }));
    expect(result.current).toEqual([]);

    unmount();
    await waitFor(() => {
      expect(tauri.unlisten).toHaveBeenCalled();
    });
  });

  it('prefers events received while the list was loading', async () => {
    let resolve: (jobs: Job[]) => void = () => undefined;
    tauri.invoke.mockReturnValue(
      new Promise<Job[]>((done) => {
        resolve = done;
      }),
    );
    const { result } = renderHook(() => useJobs());
    await waitFor(() => {
      expect(tauri.handlers.has(JOB_EVENT)).toBe(true);
    });
    emit(job({ state: 'done' }));
    await act(async () => {
      resolve([job({ state: 'queued' })]);
      await Promise.resolve();
    });
    expect(result.current).toEqual([]);
  });

  it('stays empty outside the app', () => {
    tauri.isTauri.mockReturnValue(false);
    const { result } = renderHook(() => useJobs());
    expect(result.current).toEqual([]);
    expect(tauri.invoke).not.toHaveBeenCalled();
  });
});
