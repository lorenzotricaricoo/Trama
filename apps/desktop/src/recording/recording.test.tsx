import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from '../App';
import { I18nProvider } from '../i18n/I18nProvider';
import type { Job } from '../jobs/useJobs';
import type { Meeting } from '../meetings/api';
import type { RecordingNote, RecordingState, RecordingStatus } from './api';
import { formatClock, meterFill } from './format';
import { RecordingPanel } from './RecordingPanel';

/** A core in memory, behind the same commands as `src-tauri/src/recording.rs`. */
const core = vi.hoisted(() => ({
  calls: [] as { cmd: string; args: Record<string, unknown> }[],
  handlers: new Map<string, (event: { payload: unknown }) => void>(),
  status: null as RecordingStatus | null,
  /** The error code the next start fails with. */
  startError: null as string | null,
  meeting: null as Meeting | null,
  notes: [] as RecordingNote[],
}));

const MEETING_ID = 'meeting-1';

function emit(name: string, payload: unknown) {
  act(() => core.handlers.get(name)?.({ payload }));
}

vi.mock('@tauri-apps/api/core', () => ({
  isTauri: () => true,
  invoke: vi.fn((cmd: string, args: Record<string, unknown> = {}) => {
    core.calls.push({ cmd, args });
    switch (cmd) {
      case 'active_jobs':
        return Promise.resolve([]);
      case 'set_menu_state':
        return Promise.resolve(null);
      case 'recording_status':
        return Promise.resolve(core.status);
      case 'start_recording': {
        if (core.startError) return Promise.reject(core.startError);
        core.status = { meetingId: MEETING_ID, startedAt: 0, elapsedMs: 0 };
        return Promise.resolve(core.status);
      }
      case 'stop_recording': {
        core.status = null;
        const state: RecordingState = { recording: null, stoppedMeetingId: MEETING_ID };
        // The event reaches every window, as the shell sends it.
        setTimeout(() => emit('recording-state', state), 0);
        return Promise.resolve(MEETING_ID);
      }
      case 'get_meeting':
        return core.meeting ? Promise.resolve(core.meeting) : Promise.reject(new Error('missing'));
      case 'source_summary':
        return Promise.resolve(null);
      case 'recording_notes':
        return Promise.resolve([...core.notes]);
      case 'add_recording_note': {
        const note = {
          id: `n${core.notes.length + 1}`,
          meetingId: MEETING_ID,
          atMs: 65_000,
          text: String(args.text),
        };
        core.notes.push(note);
        return Promise.resolve(note);
      }
      case 'set_recording_panel_compact':
      case 'show_recording_panel':
        return Promise.resolve(null);
      default:
        return Promise.reject(new Error(`unexpected command ${cmd}`));
    }
  }),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((name: string, handler: (event: { payload: unknown }) => void) => {
    core.handlers.set(name, handler);
    return Promise.resolve(() => {});
  }),
}));

const commands = (cmd: string) => core.calls.filter((call) => call.cmd === cmd);

function renderWith(element: React.ReactElement) {
  const user = userEvent.setup();
  render(<I18nProvider locale="en">{element}</I18nProvider>);
  return user;
}

function meeting(overrides: Partial<Meeting> = {}): Meeting {
  return {
    id: MEETING_ID,
    title: '',
    occurredAt: Date.UTC(2026, 10, 28, 14, 0),
    status: 'processing',
    durationMs: 3 * 60_000,
    errorCode: null,
    segments: [],
    language: null,
    audio: [],
    audioDeleted: false,
    ...overrides,
  };
}

function job(overrides: Partial<Job>): Job {
  return {
    id: 'job-1',
    kind: 'transcribe',
    targetId: MEETING_ID,
    state: 'running',
    attempts: 1,
    maxAttempts: 5,
    errorCode: null,
    progress: null,
    ...overrides,
  };
}

beforeEach(() => {
  core.calls.length = 0;
  core.handlers.clear();
  core.status = null;
  core.startError = null;
  core.meeting = null;
  core.notes = [];
  localStorage.clear();
});

describe('Recording (flow F2)', () => {
  it('records from the sidebar, stops, and opens the meeting as it gets transcribed', async () => {
    const user = renderWith(<App />);
    await user.click(screen.getByRole('button', { name: 'Record meeting' }));
    expect(commands('start_recording')).toHaveLength(1);
    // While recording the button stops, and the top bar shows the time.
    const stop = await screen.findByRole('button', { name: 'Stop recording' });
    expect(stop).toHaveTextContent('Recording · 00:00');
    expect(
      screen.getByRole('button', { name: 'Show the recording window · 00:00' }),
    ).toBeInTheDocument();

    core.meeting = meeting();
    core.notes = [{ id: 'n1', meetingId: MEETING_ID, atMs: 2_000, text: 'Ask about roles' }];
    await user.click(stop);
    expect(commands('stop_recording')).toHaveLength(1);
    expect(
      await screen.findByRole('heading', { level: 1, name: /^Meeting of / }),
    ).toBeInTheDocument();
    expect(screen.getByText('3 minutes', { exact: false })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Record meeting' })).toBeInTheDocument();
    // The notes are there before the transcript.
    expect(screen.getByText('Ask about roles')).toBeInTheDocument();

    // The transcription runs, then the transcript arrives with the note in its place.
    emit('job', job({ progress: 0.42 }));
    expect(await screen.findByRole('progressbar', { name: 'Transcribing' })).toHaveAttribute(
      'aria-valuenow',
      '42',
    );
    core.meeting = meeting({
      status: 'ready',
      title: 'Roadmap sync',
      segments: [
        { id: 1, role: 'others', startMs: 0, endMs: 1_500, text: 'Good morning' },
        { id: 2, role: 'me', startMs: 3_000, endMs: 4_000, text: 'Let us start' },
      ],
    });
    emit('job', job({ state: 'done', progress: 1 }));
    expect(
      await screen.findByRole('heading', { level: 1, name: 'Roadmap sync' }),
    ).toBeInTheDocument();
    const lines = within(screen.getByRole('list')).getAllByRole('listitem');
    expect(lines.map((line) => line.textContent)).toEqual([
      '00:00OthersGood morning',
      '00:02NoteAsk about roles',
      '00:03MeLet us start',
    ]);
  });

  it('explains why the recording could not start', async () => {
    core.startError = 'microphone_denied';
    const user = renderWith(<App />);
    await user.keyboard('{Meta>}{Alt>}r{/Alt}{/Meta}');
    expect(await screen.findByRole('status')).toHaveTextContent('Trama cannot use the microphone');
    expect(screen.getByRole('button', { name: 'Record meeting' })).toBeInTheDocument();

    core.startError = 'something_new';
    await user.click(screen.getByRole('button', { name: 'Record meeting' }));
    await waitFor(() =>
      expect(screen.getByRole('status')).toHaveTextContent('The recording failed (something_new).'),
    );
  });
});

describe('Recording panel', () => {
  beforeEach(() => {
    core.status = { meetingId: MEETING_ID, startedAt: 0, elapsedMs: 61_000 };
  });

  it('pins notes to the minute and shows the levels', async () => {
    const user = renderWith(<RecordingPanel />);
    expect(await screen.findByRole('timer')).toHaveTextContent('01:01');
    const input = screen.getByRole('textbox', { name: 'Note' });
    await user.type(input, '  Decision: OAuth  {Enter}');
    expect(commands('add_recording_note')).toEqual([
      { cmd: 'add_recording_note', args: { text: 'Decision: OAuth' } },
    ]);
    const notes = await screen.findByRole('list', { name: 'Notes' });
    expect(within(notes).getByRole('listitem')).toHaveTextContent('01:05Decision: OAuth');
    expect(input).toHaveValue('');
    // An empty note is not sent.
    await user.type(input, '   {Enter}');
    expect(commands('add_recording_note')).toHaveLength(1);

    await waitFor(() => expect(core.handlers.has('recording-level')).toBe(true));
    emit('recording-level', { source: 'system', peakDb: -6, rmsDb: -30 });
    const meters = document.querySelectorAll<HTMLElement>('.rec-level');
    expect(meters[1]?.style.getPropertyValue('--fill')).toBe('0.5');
  });

  it('warns about a silent track until it sounds', async () => {
    renderWith(<RecordingPanel />);
    await waitFor(() => expect(core.handlers.has('recording-notice')).toBe(true));
    emit('recording-notice', { source: 'microphone', kind: 'problem', code: 'microphone_silent' });
    expect(await screen.findByText(/No sound from the microphone/)).toBeInTheDocument();
    emit('recording-notice', { source: 'microphone', kind: 'started', code: null });
    await waitFor(() =>
      expect(screen.queryByText(/No sound from the microphone/)).not.toBeInTheDocument(),
    );
  });

  it('shrinks to a pill and stops', async () => {
    const user = renderWith(<RecordingPanel />);
    await user.click(await screen.findByRole('button', { name: 'Reduce' }));
    expect(commands('set_recording_panel_compact')).toEqual([
      { cmd: 'set_recording_panel_compact', args: { compact: true } },
    ]);
    expect(screen.getByRole('timer')).toHaveTextContent('01:01');
    await user.click(screen.getByRole('button', { name: 'Expand' }));
    expect(commands('set_recording_panel_compact').at(-1)?.args).toEqual({ compact: false });
    await user.keyboard('{Meta>}{Alt>}r{/Alt}{/Meta}');
    expect(commands('stop_recording')).toHaveLength(1);
  });
});

describe('Recording format', () => {
  it('shows the time as a clock', () => {
    expect(formatClock(0)).toBe('00:00');
    expect(formatClock(61_999)).toBe('01:01');
    expect(formatClock(3_725_000)).toBe('1:02:05');
  });

  it('maps levels to the meters', () => {
    expect(meterFill(-120)).toBe(0);
    expect(meterFill(-30)).toBe(0.5);
    expect(meterFill(3)).toBe(1);
    expect(meterFill(Number.NaN)).toBe(0);
  });
});
