import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { I18nProvider } from '../i18n/I18nProvider';
import type { Job } from '../jobs/useJobs';
import type { SearchResult, SourceRow } from '../notes/api';
import { CommandBar, type CommandBarState } from '../shell/CommandBar';
import { HomeScreen } from './HomeScreen';
import { SourcesScreen } from './SourcesScreen';

/** A core in memory, behind the same commands as `notes.rs`. */
const core = vi.hoisted(() => ({
  calls: [] as { cmd: string; args: Record<string, unknown> }[],
  rows: [] as SourceRow[],
  results: [] as SearchResult[],
}));

vi.mock('@tauri-apps/api/core', () => ({
  isTauri: () => true,
  invoke: vi.fn((cmd: string, args: Record<string, unknown> = {}) => {
    core.calls.push({ cmd, args });
    switch (cmd) {
      case 'list_sources': {
        const kind = args.kind as string | null;
        const rows = core.rows.filter((row) => !kind || row.kind === kind);
        return Promise.resolve({
          sources: rows.slice(0, Number(args.limit ?? 100)),
          next: null,
        });
      }
      case 'source_counts':
        return Promise.resolve({
          meetings: core.rows.filter((row) => row.kind === 'meeting').length,
          notes: core.rows.filter((row) => row.kind === 'note').length,
        });
      case 'search_sources':
        return Promise.resolve(core.results.slice(0, Number(args.limit ?? 100)));
      default:
        return Promise.reject(new Error(`unexpected command ${cmd}`));
    }
  }),
}));

const NOW = Date.now();

function row(overrides: Partial<SourceRow> & Pick<SourceRow, 'id' | 'kind'>): SourceRow {
  return {
    title: '',
    occurredAt: NOW,
    updatedAt: NOW,
    preview: null,
    status: overrides.kind === 'meeting' ? 'ready' : null,
    durationMs: null,
    ...overrides,
  };
}

const SYNC = row({ id: 'm1', kind: 'meeting', title: 'Roadmap sync', durationMs: 48 * 60_000 });
const BUSY = row({ id: 'm2', kind: 'meeting', title: 'Weekly', status: 'processing' });
const NOTE = row({ id: 'n1', kind: 'note', preview: 'Ideas for the launch' });

const commands = (cmd: string) => core.calls.filter((call) => call.cmd === cmd);

beforeEach(() => {
  core.calls.length = 0;
  core.rows = [SYNC, BUSY, NOTE];
  core.results = [];
});

function showSources(jobs: Job[] = []) {
  const user = userEvent.setup();
  const onOpenMeeting = vi.fn();
  const onOpenNote = vi.fn();
  render(
    <I18nProvider locale="en">
      <SourcesScreen
        jobs={jobs}
        onOpenMeeting={onOpenMeeting}
        onOpenNote={onOpenNote}
        onNewNote={vi.fn()}
      />
    </I18nProvider>,
  );
  return { user, onOpenMeeting, onOpenNote };
}

describe('Sources', () => {
  it('lists meetings and notes with their status and length, and filters them', async () => {
    const { user, onOpenNote } = showSources();
    const list = await screen.findByRole('grid', { name: 'Sources' });
    const rows = within(list).getAllByRole('row');
    expect(rows.map((r) => r.textContent)).toEqual([
      expect.stringContaining('Roadmap sync'),
      expect.stringContaining('WeeklyProcessing'),
      expect.stringContaining('Ideas for the launch'),
    ]);
    expect(rows[0]).toHaveTextContent('48 min');
    expect(screen.getByRole('radio', { name: /^All\s*3$/ })).toBeChecked();
    expect(screen.getByText('3 sources')).toBeInTheDocument();

    await user.click(screen.getByRole('radio', { name: /^Notes\s*1$/ }));
    expect(commands('list_sources').at(-1)?.args.kind).toBe('note');
    await waitFor(() =>
      expect(
        within(screen.getByRole('grid', { name: 'Sources' })).getAllByRole('row'),
      ).toHaveLength(1),
    );
    await user.dblClick(screen.getByRole('row', { name: 'Ideas for the launch' }));
    expect(onOpenNote).toHaveBeenCalledWith('n1');
  });

  it('finds the words said in a meeting and opens it at that minute', async () => {
    core.results = [
      {
        source: SYNC,
        hits: [
          { kind: 'transcript', excerpt: 'we chose \u0002OAuth\u0003 for login', atMs: 65_000 },
          { kind: 'recordingNote', excerpt: '\u0002OAuth\u0003 provider?', atMs: 90_000 },
        ],
      },
      {
        source: NOTE,
        hits: [{ kind: 'note', excerpt: 'notes on \u0002OAuth\u0003', atMs: null }],
      },
    ];
    const { user, onOpenMeeting, onOpenNote } = showSources();
    await screen.findByRole('grid', { name: 'Sources' });
    await user.type(screen.getByRole('searchbox', { name: 'Search sources' }), 'oauth');

    const results = await screen.findByRole('grid', { name: 'Search results' });
    expect(within(results).getAllByRole('row')).toHaveLength(2);
    expect(within(results).getAllByText('OAuth')[0]?.tagName).toBe('MARK');
    expect(commands('search_sources').at(-1)?.args).toEqual({
      query: 'oauth',
      kind: null,
      limit: null,
    });
    expect(screen.getByText('2 results')).toBeInTheDocument();
    expect(screen.getByRole('radio', { name: /^Meetings\s*1$/ })).toBeInTheDocument();

    await user.click(
      within(results).getByRole('button', { name: 'Open at 01:30: OAuth provider?' }),
    );
    expect(onOpenMeeting).toHaveBeenLastCalledWith('m1', 90_000);

    // The row opens at its first moment.
    await user.dblClick(within(results).getByRole('row', { name: 'Roadmap sync' }));
    expect(onOpenMeeting).toHaveBeenLastCalledWith('m1', 65_000);

    await user.click(screen.getByRole('radio', { name: /^Notes\s*1$/ }));
    const notes = screen.getByRole('grid', { name: 'Search results' });
    expect(within(notes).getAllByRole('row')).toHaveLength(1);
    await user.dblClick(within(notes).getByRole('row'));
    expect(onOpenNote).toHaveBeenCalledWith('n1');
  });

  it('says when nothing is found, and clears the search', async () => {
    const { user } = showSources();
    await screen.findByRole('grid', { name: 'Sources' });
    await user.type(screen.getByRole('searchbox', { name: 'Search sources' }), 'zebra');
    expect(await screen.findByText('Nothing found for “zebra”')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Clear filters' }));
    expect(await screen.findByRole('grid', { name: 'Sources' })).toBeInTheDocument();
    expect(screen.getByRole('searchbox', { name: 'Search sources' })).toHaveValue('');
  });

  it('invites to start when the library is empty', async () => {
    core.rows = [];
    showSources();
    expect(
      await screen.findByRole('heading', { name: 'Everything you bring into Trama lands here' }),
    ).toBeInTheDocument();
  });
});

describe('Home', () => {
  function showHome(jobs: Job[] = []) {
    const user = userEvent.setup();
    const onOpenMeeting = vi.fn();
    const onSources = vi.fn();
    render(
      <I18nProvider locale="en">
        <HomeScreen
          jobs={jobs}
          onOpenMeeting={onOpenMeeting}
          onOpenNote={vi.fn()}
          onSources={onSources}
          onNewNote={vi.fn()}
        />
      </I18nProvider>,
    );
    return { user, onOpenMeeting, onSources };
  }

  it('shows the latest meetings, the recent notes and what is being processed', async () => {
    const running: Job = {
      id: 'j1',
      kind: 'transcribe',
      targetId: 'm2',
      state: 'running',
      attempts: 1,
      maxAttempts: 5,
      errorCode: null,
      progress: 0.62,
    };
    const { user, onOpenMeeting, onSources } = showHome([running]);
    expect(await screen.findByRole('heading', { level: 1, name: 'Overview' })).toBeInTheDocument();
    const meetings = await screen.findByRole('grid', { name: /Latest meetings/ });
    expect(within(meetings).getAllByRole('row')).toHaveLength(2);
    expect(within(meetings).getByRole('row', { name: 'Roadmap sync' })).toHaveTextContent('48 min');
    const notes = screen.getByRole('grid', { name: /Recent notes/ });
    expect(within(notes).getByRole('row')).toHaveTextContent('Ideas for the launch');

    const status = screen.getByRole('status');
    expect(status).toHaveTextContent('Weekly');
    expect(status).toHaveTextContent('Transcribing');
    expect(status).toHaveTextContent('62%');

    await user.dblClick(within(meetings).getByRole('row', { name: 'Roadmap sync' }));
    expect(onOpenMeeting).toHaveBeenCalledWith('m1');
    await user.click(screen.getByRole('button', { name: 'All sources' }));
    expect(onSources).toHaveBeenCalled();
  });

  it('invites to start when there is nothing yet', async () => {
    core.rows = [];
    showHome();
    expect(
      await screen.findByRole('heading', { name: 'Your overview starts here' }),
    ).toBeInTheDocument();
  });
});

describe('Command bar search', () => {
  it('finds sources while typing and opens a meeting at the moment found', async () => {
    core.results = [
      {
        source: SYNC,
        hits: [{ kind: 'transcript', excerpt: 'the \u0002budget\u0003 is approved', atMs: 65_000 }],
      },
    ];
    const user = userEvent.setup();
    const onOpenSource = vi.fn();
    const bar: CommandBarState = {
      state: 'open',
      isOpen: true,
      open: vi.fn(),
      close: vi.fn(),
      toggle: vi.fn(),
      closed: vi.fn(),
    };
    render(
      <I18nProvider locale="en">
        <CommandBar bar={bar} commands={[]} onOpenSource={onOpenSource} />
      </I18nProvider>,
    );
    await user.type(screen.getByRole('textbox', { name: 'Type a command' }), 'b');
    // One letter searches nothing.
    await act(() => new Promise((resolve) => setTimeout(resolve, 250)));
    expect(commands('search_sources')).toHaveLength(0);

    await user.type(screen.getByRole('textbox', { name: 'Type a command' }), 'udget');
    const item = await screen.findByRole('menuitem', { name: /Roadmap sync/ });
    expect(item).toHaveTextContent('01:05');
    expect(commands('search_sources').at(-1)?.args).toEqual({
      query: 'budget',
      kind: null,
      limit: 6,
    });
    await user.keyboard('{Enter}');
    expect(onOpenSource).toHaveBeenCalledWith(SYNC, 65_000);
    expect(bar.close).toHaveBeenCalled();
  });
});
