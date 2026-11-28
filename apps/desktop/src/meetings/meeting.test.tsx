import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { I18nProvider } from '../i18n/I18nProvider';
import type { Job } from '../jobs/useJobs';
import type { RecordingNote } from '../recording/api';
import type { KeyPoint, Meeting, Summary } from './api';
import MeetingScreen from './MeetingScreen';

/** A core in memory, behind the same commands as `meetings.rs` and `summary.rs`. */
const core = vi.hoisted(() => ({
  calls: [] as { cmd: string; args: Record<string, unknown> }[],
  meeting: null as Meeting | null,
  summary: null as Summary | null,
  notes: [] as RecordingNote[],
}));

const MEETING_ID = 'meeting-1';

vi.mock('@tauri-apps/api/core', () => ({
  isTauri: () => true,
  convertFileSrc: (path: string) => `asset://localhost${path}`,
  invoke: vi.fn((cmd: string, args: Record<string, unknown> = {}) => {
    core.calls.push({ cmd, args });
    switch (cmd) {
      case 'get_meeting':
        return Promise.resolve(core.meeting);
      case 'source_summary':
        return Promise.resolve(core.summary);
      case 'recording_notes':
        return Promise.resolve([...core.notes]);
      case 'library_settings':
        return Promise.resolve({ audioRetention: 'never' });
      case 'set_source_title': {
        const title = String(args.title);
        return Promise.resolve({
          sourceId: MEETING_ID,
          title: title || 'Generated title',
          titleOrigin: title ? 'user' : 'auto',
        });
      }
      case 'edit_summary':
        return Promise.resolve({
          ...core.summary,
          body: args.body,
          keyPoints: (args.keyPoints as KeyPoint[]).filter((point) => point.text.trim()),
          model: 'user',
        });
      case 'retranscribe':
      case 'retry_job':
        return Promise.resolve({ id: 'job-2' });
      default:
        return Promise.reject(new Error(`unexpected command ${cmd}`));
    }
  }),
}));

const commands = (cmd: string) => core.calls.filter((call) => call.cmd === cmd);

function meeting(overrides: Partial<Meeting> = {}): Meeting {
  return {
    id: MEETING_ID,
    title: 'Roadmap sync',
    occurredAt: Date.UTC(2026, 10, 28, 14, 0),
    status: 'ready',
    durationMs: 48 * 60_000,
    errorCode: null,
    language: 'it',
    segments: [
      { id: 1, role: 'others', startMs: 0, endMs: 1_500, text: 'Good morning' },
      { id: 2, role: 'me', startMs: 3_000, endMs: 4_000, text: 'Let us start' },
      { id: 3, role: 'others', startMs: 9_000, endMs: 11_000, text: 'OAuth it is' },
    ],
    audio: [],
    audioDeleted: false,
    ...overrides,
  };
}

const SUMMARY: Summary = {
  id: 'summary-1',
  title: 'Roadmap sync',
  body: 'We chose OAuth.',
  keyPoints: [
    { text: 'OAuth with the company provider', atMs: 9_000 },
    { text: 'Roles still open', atMs: null },
  ],
  model: 'some-model',
};

function show(jobs: Job[] = []) {
  const user = userEvent.setup();
  const onNotice = vi.fn();
  render(
    <I18nProvider locale="en">
      <MeetingScreen meetingId={MEETING_ID} jobs={jobs} onNotice={onNotice} />
    </I18nProvider>,
  );
  return { user, onNotice };
}

beforeEach(() => {
  core.calls.length = 0;
  core.meeting = meeting();
  core.summary = SUMMARY;
  core.notes = [];
  HTMLMediaElement.prototype.play = vi.fn(() => Promise.resolve());
  HTMLMediaElement.prototype.pause = vi.fn();
});

describe('Meeting page (flow F4)', () => {
  it('shows the date, length and language', async () => {
    show();
    expect(
      await screen.findByRole('heading', { level: 1, name: 'Roadmap sync' }),
    ).toBeInTheDocument();
    expect(screen.getByText(/48 minutes · Italian$/)).toBeInTheDocument();
  });

  it('renames the meeting, and an empty title gives it back to the summary', async () => {
    const { user } = show();
    await user.click(await screen.findByRole('button', { name: 'Roadmap sync' }));
    const field = screen.getByRole('textbox', { name: 'Meeting title' });
    await user.clear(field);
    await user.type(field, '  Kick-off   Q1 {Enter}');
    expect(commands('set_source_title').at(-1)?.args).toEqual({
      sourceId: MEETING_ID,
      title: 'Kick-off Q1',
    });
    expect(
      await screen.findByRole('heading', { level: 1, name: 'Kick-off Q1' }),
    ).toBeInTheDocument();

    // Escape leaves the title as it was.
    await user.click(screen.getByRole('button', { name: 'Kick-off Q1' }));
    await user.type(screen.getByRole('textbox', { name: 'Meeting title' }), 'zzz{Escape}');
    expect(commands('set_source_title')).toHaveLength(1);

    await user.click(screen.getByRole('button', { name: 'Kick-off Q1' }));
    await user.clear(screen.getByRole('textbox', { name: 'Meeting title' }));
    await user.keyboard('{Enter}');
    expect(commands('set_source_title').at(-1)?.args).toEqual({ sourceId: MEETING_ID, title: '' });
    expect(
      await screen.findByRole('heading', { level: 1, name: 'Generated title' }),
    ).toBeInTheDocument();
  });

  it('edits the summary and its key points', async () => {
    const { user } = show();
    expect(await screen.findByText('generated · editable')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Edit' }));
    const body = screen.getByRole('textbox', { name: 'Summary text' });
    await user.clear(body);
    await user.type(body, 'We chose OAuth, SAML as fallback.');
    await user.click(screen.getAllByRole('button', { name: 'Remove the key point' })[1]!);
    await user.click(screen.getByRole('button', { name: 'Add a key point' }));
    await user.type(screen.getByRole('textbox', { name: 'Key point 2' }), 'Credentials by Friday');
    await user.click(screen.getByRole('button', { name: 'Save' }));

    expect(commands('edit_summary')).toEqual([
      {
        cmd: 'edit_summary',
        args: {
          sourceId: MEETING_ID,
          body: 'We chose OAuth, SAML as fallback.',
          keyPoints: [
            { text: 'OAuth with the company provider', atMs: 9_000 },
            { text: 'Credentials by Friday', atMs: null },
          ],
        },
      },
    ]);
    expect(await screen.findByText('edited by you')).toBeInTheDocument();
    expect(screen.getByText('We chose OAuth, SAML as fallback.')).toBeInTheDocument();
    const points = within(screen.getByRole('list', { name: 'Key points' })).getAllByRole(
      'listitem',
    );
    expect(points.map((point) => point.textContent)).toEqual([
      'OAuth with the company provider00:09',
      'Credentials by Friday',
    ]);
  });

  it('cancels an edit without saving', async () => {
    const { user } = show();
    await user.click(await screen.findByRole('button', { name: 'Edit' }));
    await user.type(screen.getByRole('textbox', { name: 'Summary text' }), ' more');
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(commands('edit_summary')).toHaveLength(0);
    expect(screen.getByText('We chose OAuth.')).toBeInTheDocument();
  });

  it('plays the two tracks together and jumps to a key point', async () => {
    core.meeting = meeting({
      audio: [
        { role: 'me', path: '/lib/audio/m/me.wav' },
        { role: 'others', path: '/lib/audio/m/others.wav' },
      ],
    });
    const { user } = show();
    const player = await screen.findByRole('group', { name: 'Meeting audio' });
    const tracks = document.querySelectorAll('audio');
    expect([...tracks].map((track) => track.getAttribute('src'))).toEqual([
      'asset://localhost/lib/audio/m/me.wav',
      'asset://localhost/lib/audio/m/others.wav',
    ]);

    await user.click(within(player).getByRole('button', { name: 'Play' }));
    expect(HTMLMediaElement.prototype.play).toHaveBeenCalledTimes(2);

    const keyPoints = screen.getByRole('list', { name: 'Key points' });
    await user.click(within(keyPoints).getByRole('button', { name: 'Go to 00:09' }));
    expect([...tracks].map((track) => track.currentTime)).toEqual([9, 9]);
    const line = screen.getByText('OAuth it is').closest('li');
    expect(line).toHaveAttribute('data-flash');
    expect(within(player).getByText('00:09 / 48:00')).toBeInTheDocument();

    // The transcript timestamps move the player too.
    await user.click(screen.getByRole('button', { name: 'Go to 00:03' }));
    expect(tracks[1]?.currentTime).toBe(3);
  });

  it('has no player once the audio is gone, and says why', async () => {
    core.meeting = meeting({ audioDeleted: true });
    show();
    expect(await screen.findByText('audio not kept · setting “Never”')).toBeInTheDocument();
    expect(screen.queryByRole('group', { name: 'Meeting audio' })).not.toBeInTheDocument();
    // Without audio the timestamps of the transcript are plain text.
    expect(screen.queryByRole('button', { name: 'Go to 00:03' })).not.toBeInTheDocument();
  });

  it('transcribes again a meeting that failed without a job to retry', async () => {
    core.summary = null;
    core.meeting = meeting({
      status: 'failed',
      errorCode: 'recording_interrupted',
      segments: [],
      audio: [{ role: 'me', path: '/lib/audio/m/me.wav' }],
    });
    const { user } = show();
    expect(await screen.findByRole('alert')).toHaveTextContent('The app closed while recording');
    await user.click(screen.getByRole('button', { name: 'Try again' }));
    expect(commands('retranscribe')).toEqual([{ cmd: 'retranscribe', args: { id: MEETING_ID } }]);
  });

  it('retries the failed job when there is one, and nothing without audio', async () => {
    core.summary = null;
    core.meeting = meeting({ status: 'failed', errorCode: 'model_missing', segments: [] });
    const failed: Job = {
      id: 'job-1',
      kind: 'transcribe',
      targetId: MEETING_ID,
      state: 'failed',
      attempts: 5,
      maxAttempts: 5,
      errorCode: 'model_missing',
      progress: null,
    };
    const { user } = show([failed]);
    await user.click(await screen.findByRole('button', { name: 'Try again' }));
    expect(commands('retry_job')).toEqual([{ cmd: 'retry_job', args: { id: 'job-1' } }]);
    expect(commands('retranscribe')).toHaveLength(0);
  });

  it('offers no retry once the audio is missing', async () => {
    core.summary = null;
    core.meeting = meeting({ status: 'failed', errorCode: 'audio_missing', segments: [] });
    show();
    expect(await screen.findByRole('alert')).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.queryByRole('button', { name: 'Try again' })).not.toBeInTheDocument(),
    );
  });

  it('copies the summary as text', async () => {
    const { user, onNotice } = show();
    const writeText = vi.spyOn(navigator.clipboard, 'writeText').mockResolvedValue();
    await user.click(await screen.findByRole('button', { name: 'Copy summary' }));
    expect(writeText).toHaveBeenCalledWith(
      'Roadmap sync\n\nWe chose OAuth.\n\n- OAuth with the company provider (00:09)\n- Roles still open',
    );
    await waitFor(() => expect(onNotice).toHaveBeenCalledWith('Summary copied.'));
  });
});
