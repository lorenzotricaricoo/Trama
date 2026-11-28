import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { I18nProvider } from '../i18n/I18nProvider';
import type { Job } from '../jobs/useJobs';
import { JobIndicator } from './JobIndicator';

function job(id: string, change: Partial<Job>): Job {
  return {
    id,
    kind: 'transcribe',
    targetId: 'source',
    state: 'running',
    attempts: 1,
    maxAttempts: 3,
    errorCode: null,
    progress: null,
    ...change,
  };
}

function renderWith(jobs: Job[]) {
  return render(
    <I18nProvider locale="en">
      <JobIndicator jobs={jobs} />
    </I18nProvider>,
  );
}

describe('JobIndicator', () => {
  it('is hidden without active jobs', () => {
    renderWith([job('a', { state: 'failed' })]);
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });

  it('shows the running job, its progress and how many others wait', () => {
    renderWith([
      job('a', { state: 'queued', kind: 'summarize' }),
      job('b', { progress: 0.62 }),
      job('c', { state: 'queued' }),
    ]);
    const status = screen.getByRole('status');
    expect(status).toHaveTextContent('Transcribing');
    expect(status).toHaveTextContent('62%');
    expect(status).toHaveTextContent('+2');
  });

  it('shows a queued job as waiting', () => {
    renderWith([job('a', { state: 'queued' })]);
    expect(screen.getByRole('status')).toHaveTextContent('Waiting');
  });
});
