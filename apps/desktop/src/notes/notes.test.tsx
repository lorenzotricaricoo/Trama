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
}));

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
      case 'create_note': {
        const id = `note-${core.notes.size + 1}`;
        core.notes.set(id, { body: String(args.body), createdAt: Date.UTC(2026, 10, 27, 9, 30) });
        return Promise.resolve(noteOf(id));
      }
      case 'save_note': {
        const id = String(args.id);
        core.notes.set(id, { ...noteOf(id), body: String(args.body) });
        return Promise.resolve({ versionNumber: 1, savedAt: 0 });
      }
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
  localStorage.clear();
});

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
    expect(commands('create_note')[0]?.args).toEqual({ body: '# Weekly sync' });
    expect(await screen.findByText('Saved')).toBeInTheDocument();

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
    await waitFor(async () => {
      const view = EditorView.findFromDOM(await editor());
      expect(view?.state.doc.toString()).toBe('# Weekly sync\n\nperché no');
    });
    // Opening a note does not write it again.
    expect(commands('save_note')).toHaveLength(1);
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
    await waitFor(() => expect(core.handlers.has(QUIT_REQUESTED_EVENT)).toBe(true));
    act(() => core.handlers.get(QUIT_REQUESTED_EVENT)?.({ payload: null }));
    await waitFor(() => expect(commands('quit_ready')).toHaveLength(1), { timeout: 150 });
    const order = core.calls.map((call) => call.cmd).filter((cmd) => cmd !== 'active_jobs');
    expect(order).toEqual(['create_note', 'quit_ready']);
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
