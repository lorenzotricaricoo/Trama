import { EditorView } from '@codemirror/view';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from '../App';
import { I18nProvider } from '../i18n/I18nProvider';
import type { Note, SourceRow } from './api';
import { QUIT_REQUESTED_EVENT } from './flush';

/** A core in memory, behind the same commands as `src-tauri/src/notes.rs`. */
const core = vi.hoisted(() => ({
  notes: new Map<string, { body: string; createdAt: number }>(),
  calls: [] as { cmd: string; args: Record<string, unknown> }[],
  handlers: new Map<string, (event: { payload: unknown }) => void>(),
  /** Upcoming writes (create or save) that fail, like a full disk. */
  failingWrites: 0,
  /** While set, create_note waits for it, like a slow disk. */
  createGate: null as Promise<void> | null,
}));

/** Fails if a write should fail now. */
function write(): Promise<void> {
  if (core.failingWrites > 0) {
    core.failingWrites -= 1;
    return Promise.reject(new Error('disk full'));
  }
  return Promise.resolve();
}

function noteOf(id: string): Note {
  const note = core.notes.get(id);
  if (!note) throw new Error(`note ${id} not found`);
  return {
    id,
    title: '',
    body: note.body,
    versionNumber: 1,
    createdAt: note.createdAt,
    updatedAt: 0,
  };
}

vi.mock('@tauri-apps/api/core', () => ({
  isTauri: () => true,
  invoke: vi.fn((cmd: string, args: Record<string, unknown> = {}) => {
    core.calls.push({ cmd, args });
    switch (cmd) {
      case 'active_jobs':
        return Promise.resolve([]);
      case 'create_note':
        return (core.createGate ?? Promise.resolve()).then(write).then(() => {
          const id = `note-${core.notes.size + 1}`;
          core.notes.set(id, {
            body: String(args.body),
            createdAt: Date.UTC(2026, 10, 27, 9, 30),
          });
          return noteOf(id);
        });
      case 'save_note':
        return write().then(() => {
          const id = String(args.id);
          core.notes.set(id, { ...noteOf(id), body: String(args.body) });
          return { versionNumber: 1, savedAt: 0 };
        });
      case 'set_note_title':
        return write().then(() => String(args.title).trim());
      case 'get_note':
        return Promise.resolve(noteOf(String(args.id)));
      case 'list_sources': {
        const rows: SourceRow[] = [...core.notes.entries()].reverse().map(([id, note]) => ({
          id,
          kind: 'note',
          title: '',
          occurredAt: note.createdAt,
          updatedAt: 0,
          preview: note.body.replace(/^[#\s]+/, '').split('\n')[0] || null,
        }));
        return Promise.resolve({ sources: rows, next: null });
      }
      case 'quit_ready':
      case 'quit_cancelled':
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

function renderApp() {
  const user = userEvent.setup();
  render(
    <I18nProvider locale="en">
      <App />
    </I18nProvider>,
  );
  return user;
}

const commands = (cmd: string) => core.calls.filter((call) => call.cmd === cmd);
const editor = () => screen.findByRole('textbox', { name: 'Note text' });

/** Types at the end of the note, as the user would. */
async function type(text: string) {
  const content = await editor();
  const view = EditorView.findFromDOM(content);
  if (!view) throw new Error('no editor view');
  act(() => {
    view.dispatch({ changes: { from: view.state.doc.length, insert: text } });
  });
}

beforeEach(() => {
  core.notes.clear();
  core.calls.length = 0;
  core.handlers.clear();
  core.failingWrites = 0;
  core.createGate = null;
  localStorage.clear();
});

/** Asks the app to quit, as the shell does for ⌘Q and the close button. */
async function quit() {
  await waitFor(() => expect(core.handlers.has(QUIT_REQUESTED_EVENT)).toBe(true));
  act(() => core.handlers.get(QUIT_REQUESTED_EVENT)?.({ payload: null }));
}

describe('Notes (flow F3)', () => {
  it('⌘N opens an editor that saves on its own, and the note joins Sources', async () => {
    const user = renderApp();
    await user.keyboard('{Meta>}n{/Meta}');
    expect(await editor()).toHaveFocus();
    expect(screen.getByRole('heading', { level: 1, name: 'Note' })).toBeInTheDocument();
    expect(screen.getByText('Saved automatically')).toBeInTheDocument();
    // Nothing exists until something is typed.
    expect(commands('create_note')).toHaveLength(0);

    await type('# Weekly sync');
    await waitFor(() => expect(commands('create_note')).toHaveLength(1));
    expect(commands('create_note')[0]?.args).toEqual({ title: '', body: '# Weekly sync' });
    expect(await screen.findByText(/^Saved at /)).toBeInTheDocument();

    await type('\n\nperché no');
    await waitFor(() =>
      expect(commands('save_note').at(-1)?.args).toEqual({
        id: 'note-1',
        body: '# Weekly sync\n\nperché no',
      }),
    );
    expect(commands('create_note')).toHaveLength(1);

    const nav = screen.getByRole('navigation', { name: 'Main' });
    await user.click(within(nav).getByRole('link', { name: 'Sources' }));
    const list = await screen.findByRole('grid', { name: 'Sources' });
    const row = within(list).getByRole('row', { name: /Weekly sync/ });
    await user.click(row);
    // A note with text opens as it reads: Markdown rendered.
    expect(await screen.findByRole('heading', { level: 1, name: 'Weekly sync' })).toBeVisible();
    expect(screen.getByText('perché no', { selector: 'p' })).toBeVisible();
    expect(screen.getByRole('radio', { name: 'Preview' })).toBeChecked();
    await user.click(screen.getByRole('radio', { name: 'Write' }));
    await waitFor(async () => {
      const view = EditorView.findFromDOM(await editor());
      expect(view?.state.doc.toString()).toBe('# Weekly sync\n\nperché no');
    });
    // Opening a note does not write it again.
    expect(commands('save_note')).toHaveLength(1);
  });

  it('names a note with its title, and formats from the toolbar', async () => {
    const user = renderApp();
    await user.keyboard('{Meta>}n{/Meta}');
    const title = await screen.findByRole('textbox', { name: 'Note title' });
    await waitFor(() => expect(title).toBeEnabled());
    await user.type(title, 'Piano Q1');
    await waitFor(() =>
      expect(commands('create_note').at(-1)?.args).toEqual({ title: 'Piano Q1', body: '' }),
    );
    // Enter moves to the text.
    await user.type(title, '{Enter}');
    await waitFor(async () => expect(await editor()).toHaveFocus());
    await type('idea');
    const view = EditorView.findFromDOM(await editor());
    act(() => view?.dispatch({ selection: { anchor: 0, head: 4 } }));
    await user.click(screen.getByRole('button', { name: 'Bold' }));
    expect(view?.state.doc.toString()).toBe('**idea**');
    await waitFor(() =>
      expect(commands('save_note').at(-1)?.args).toEqual({ id: 'note-1', body: '**idea**' }),
    );
    await user.type(title, ' bis');
    await waitFor(() =>
      expect(commands('set_note_title').at(-1)?.args).toEqual({
        id: 'note-1',
        title: 'Piano Q1 bis',
      }),
    );
    // Side by side: the text and its preview.
    await user.click(screen.getByRole('radio', { name: 'Side by side' }));
    expect(await editor()).toBeVisible();
    expect(screen.getByText('idea', { selector: 'strong' })).toBeVisible();
  });

  it('writes at once when the editor loses focus, without waiting for the pause', async () => {
    const user = renderApp();
    await user.keyboard('{Meta>}n{/Meta}');
    await type('Draft');
    fireEvent.blur(await editor());
    await waitFor(() => expect(commands('create_note')).toHaveLength(1), { timeout: 150 });
  });

  it('writes what is pending before telling the shell to quit', async () => {
    const user = renderApp();
    await user.keyboard('{Meta>}n{/Meta}');
    await type('Last words');
    await quit();
    await waitFor(() => expect(commands('quit_ready')).toHaveLength(1), { timeout: 150 });
    // Only the note and quit commands matter here, not what the shell loads at start.
    const order = core.calls
      .map((call) => call.cmd)
      .filter((cmd) => cmd === 'create_note' || cmd === 'save_note' || cmd.startsWith('quit_'));
    expect(order).toEqual(['create_note', 'quit_ready']);
  });

  it('stays open when a save fails at quit, and quits on a second request', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    const user = renderApp();
    await user.keyboard('{Meta>}n{/Meta}');
    await type('Keep me');
    core.failingWrites = 1;
    await quit();
    await waitFor(() => expect(commands('quit_cancelled')).toHaveLength(1));
    expect(commands('quit_ready')).toHaveLength(0);
    expect(await screen.findByText(/A note is not saved yet/)).toBeInTheDocument();
    expect(screen.getByRole('alert')).toHaveTextContent('Not saved yet: trying again.');
    // The text is still on screen.
    const view = EditorView.findFromDOM(await editor());
    expect(view?.state.doc.toString()).toBe('Keep me');

    core.failingWrites = 1;
    await quit();
    await waitFor(() => expect(commands('quit_ready')).toHaveLength(1));
    error.mockRestore();
  });

  it('keeps a failed save of a closed note in the quit until it lands', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    const user = renderApp();
    await user.keyboard('{Meta>}n{/Meta}');
    await type('Written on a full disk');
    core.failingWrites = 1;
    const nav = screen.getByRole('navigation', { name: 'Main' });
    await user.click(within(nav).getByRole('link', { name: 'Home' }));
    await waitFor(() => expect(commands('create_note')).toHaveLength(1));
    expect(core.notes.size).toBe(0);

    // The editor is gone, but quitting still writes its text first.
    await quit();
    await waitFor(() => expect(commands('quit_ready')).toHaveLength(1));
    expect(commands('quit_cancelled')).toHaveLength(0);
    expect([...core.notes.values()].map((note) => note.body)).toEqual(['Written on a full disk']);
    error.mockRestore();
  });

  it('going Back to a new note while it is being created opens that note', async () => {
    let open = () => {};
    core.createGate = new Promise((resolve) => (open = resolve));
    const user = renderApp();
    await user.keyboard('{Meta>}n{/Meta}');
    await type('Slow start');
    const nav = screen.getByRole('navigation', { name: 'Main' });
    await user.click(within(nav).getByRole('link', { name: 'Home' }));
    await screen.findByRole('heading', { level: 1, name: 'Welcome to Trama' });
    await waitFor(() =>
      expect(screen.queryByRole('textbox', { name: 'Note text' })).not.toBeInTheDocument(),
    );
    expect(commands('create_note')).toHaveLength(1);
    await user.click(screen.getByRole('button', { name: 'Back' }));
    // Still creating: the screen waits instead of offering an empty new note.
    expect(screen.queryByRole('textbox', { name: 'Note text' })).not.toBeInTheDocument();
    act(() => open());
    // A note with text opens as it reads; Write shows the text again.
    await user.click(await screen.findByRole('radio', { name: 'Write' }));
    await waitFor(async () => {
      const view = EditorView.findFromDOM(await editor());
      expect(view?.state.doc.toString()).toBe('Slow start');
    });
    await type(', then more');
    await waitFor(() =>
      expect(commands('save_note').at(-1)?.args).toEqual({
        id: 'note-1',
        body: 'Slow start, then more',
      }),
    );
    expect(commands('create_note')).toHaveLength(1);
    expect(core.notes.size).toBe(1);
  });

  it('⌘N follows the key on Latin layouts and the physical key on others', async () => {
    renderApp();
    // Dvorak: the physical N key types B, so ⌘ with it is not New note.
    fireEvent.keyDown(window, { key: 'b', code: 'KeyN', metaKey: true });
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(screen.queryByRole('textbox', { name: 'Note text' })).not.toBeInTheDocument();
    // Russian: the N key types т.
    fireEvent.keyDown(window, { key: 'т', code: 'KeyN', metaKey: true });
    expect(await editor()).toBeInTheDocument();
  });

  it('opens from the command bar, and ⌘N works while the bar is open', async () => {
    const user = renderApp();
    await user.keyboard('{Meta>}k{/Meta}');
    const bar = await screen.findByRole('dialog', { name: 'Command bar' });
    expect(within(bar).getByRole('menuitem', { name: /New note/ })).toBeInTheDocument();
    await user.keyboard('{Meta>}n{/Meta}');
    expect(await editor()).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.queryByRole('dialog', { name: 'Command bar' })).not.toBeInTheDocument(),
    );
  });

  it('leaves ⌘N alone while a character is being composed', async () => {
    renderApp();
    fireEvent.keyDown(window, { key: 'n', code: 'KeyN', metaKey: true, isComposing: true });
    expect(screen.queryByRole('textbox', { name: 'Note text' })).not.toBeInTheDocument();
    // Ctrl+N is a text-editing key on macOS (next line), not New note.
    fireEvent.keyDown(window, { key: 'n', code: 'KeyN', ctrlKey: true });
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(screen.queryByRole('textbox', { name: 'Note text' })).not.toBeInTheDocument();
  });
});
