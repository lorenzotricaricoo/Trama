import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from '../App';
import { createBundle, createTranslate } from '../i18n/catalogs';
import { I18nProvider } from '../i18n/I18nProvider';
import { MENU_COMMAND_EVENT } from '../shell/actions';
import { webview } from '../test/webview';
import type { SourceRow } from './api';
import { importMessage, type ImportResult } from './importNotes';
import { splitFrontMatter } from './NotePreview';

/** A core in memory, behind the import commands of `src-tauri/src/notes.rs`. */
const core = vi.hoisted(() => ({
  notes: [] as { id: string; title: string; body: string }[],
  calls: [] as { cmd: string; args: Record<string, unknown> }[],
  handlers: new Map<string, (event: { payload: unknown }) => void>(),
  /** What the open panel returns. */
  chosen: [] as string[],
  /** Files the import leaves out, by name. */
  skip: new Map<string, string>(),
  failImport: false,
}));

vi.mock('@tauri-apps/api/core', () => ({
  isTauri: () => true,
  invoke: vi.fn((cmd: string, args: Record<string, unknown> = {}) => {
    core.calls.push({ cmd, args });
    switch (cmd) {
      case 'active_jobs':
        return Promise.resolve([]);
      case 'set_menu_state':
      case 'recording_status':
        return Promise.resolve(null);
      case 'choose_notes_to_import':
        return Promise.resolve(core.chosen);
      case 'import_notes': {
        if (core.failImport) return Promise.reject('internal');
        const result: ImportResult = { imported: [], skipped: [] };
        for (const path of args.paths as string[]) {
          const name = path.split('/').at(-1) ?? path;
          const reason = core.skip.get(name);
          if (reason) {
            result.skipped.push({ name, reason: reason as ImportResult['skipped'][0]['reason'] });
            continue;
          }
          const note = {
            id: `note-${core.notes.length + 1}`,
            title: name.replace(/\.md$/, ''),
            body: `---\ntags: [q3]\n---\n# ${name}\n`,
          };
          core.notes.push(note);
          result.imported.push({ id: note.id, title: note.title });
        }
        return Promise.resolve(result);
      }
      case 'get_note': {
        const note = core.notes.find((n) => n.id === args.id);
        return note
          ? Promise.resolve({
              ...note,
              versionNumber: 1,
              occurredAt: Date.UTC(2025, 4, 1),
              createdAt: 0,
              updatedAt: 0,
            })
          : Promise.reject(new Error('not found'));
      }
      case 'list_sources': {
        const rows: SourceRow[] = core.notes.map((note) => ({
          id: note.id,
          kind: 'note',
          title: note.title,
          occurredAt: Date.UTC(2025, 4, 1),
          updatedAt: 0,
          preview: null,
          status: null,
          durationMs: null,
        }));
        return Promise.resolve({ sources: args.kind === 'meeting' ? [] : rows, next: null });
      }
      case 'source_counts':
        return Promise.resolve({ meetings: 0, notes: core.notes.length });
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
const position = { x: 10, y: 10 } as never;

function renderApp() {
  const user = userEvent.setup();
  render(
    <I18nProvider locale="en">
      <App />
    </I18nProvider>,
  );
  return user;
}

beforeEach(() => {
  core.notes = [];
  core.calls.length = 0;
  core.handlers.clear();
  core.chosen = [];
  core.skip.clear();
  core.failImport = false;
});

describe('splitFrontMatter', () => {
  it('sets the front matter apart from the text', () => {
    expect(splitFrontMatter('---\ntags: [q3]\ndate: 2025-05-01\n---\n# Piano\n')).toEqual({
      frontMatter: 'tags: [q3]\ndate: 2025-05-01',
      body: '# Piano\n',
    });
    expect(splitFrontMatter('---\r\na: 1\r\n---\r\ntext').body).toBe('text');
    expect(splitFrontMatter('# Piano\n\n---\nnot: front matter\n---\n').frontMatter).toBeNull();
    expect(splitFrontMatter('---\nno end').frontMatter).toBeNull();
  });
});

describe('importMessage', () => {
  const t = createTranslate(createBundle('en'));
  const it_ = createTranslate(createBundle('it'));

  it('says how many notes arrived', () => {
    expect(importMessage(t, { imported: [{ id: 'a', title: 'Piano Q3' }], skipped: [] })).toBe(
      'Imported “Piano Q3”.',
    );
    const two = {
      imported: [
        { id: 'a', title: 'a' },
        { id: 'b', title: 'b' },
      ],
      skipped: [],
    };
    expect(importMessage(t, two)).toBe('Imported 2 notes.');
    expect(importMessage(it_, two)).toBe('Importate 2 note.');
  });

  it('explains every file left out, grouped by reason', () => {
    const result: ImportResult = {
      imported: [],
      skipped: [
        { name: 'a.md', reason: 'duplicate' },
        { name: 'foto.png', reason: 'not_markdown' },
        { name: 'b.md', reason: 'too_large' },
        { name: 'c.md', reason: 'too_large' },
        { name: 'd.md', reason: 'too_large' },
        { name: 'latin1.md', reason: 'not_text' },
      ],
    };
    expect(importMessage(t, result)).toBe(
      "No notes imported. latin1.md isn't UTF-8 text. " +
        'b.md, c.md and 1 more are larger than 2 MB. ' +
        "foto.png isn't a Markdown file. a.md is already in Trama.",
    );
    expect(importMessage(it_, result)).toContain('b.md, c.md e altri 1 superano 2 MB.');
  });
});

describe('Importing Markdown files (#57)', () => {
  it('shows where to drop while files are held over the window', async () => {
    renderApp();
    await waitFor(() => expect(webview.handler).toBeDefined());
    act(() => webview.drag({ type: 'enter', paths: ['/tmp/a.md'], position }));
    expect(await screen.findByText('Drop to import as notes')).toBeInTheDocument();
    act(() => webview.drag({ type: 'leave' }));
    await waitFor(() => expect(screen.queryByText('Drop to import as notes')).toBeNull());
    expect(commands('import_notes')).toHaveLength(0);
  });

  it('turns dropped files into notes listed in Sources, saying what was skipped', async () => {
    core.skip.set('foto.png', 'not_markdown');
    renderApp();
    await waitFor(() => expect(webview.handler).toBeDefined());
    act(() => webview.drag({ type: 'enter', paths: ['/n/Piano.md'], position }));
    act(() =>
      webview.drag({
        type: 'drop',
        paths: ['/n/Piano.md', '/n/Diario.md', '/n/foto.png'],
        position,
      }),
    );
    await waitFor(() =>
      expect(commands('import_notes')[0]?.args.paths).toEqual([
        '/n/Piano.md',
        '/n/Diario.md',
        '/n/foto.png',
      ]),
    );
    const list = await screen.findByRole('grid', { name: 'Sources' });
    expect(await within(list).findByText('Piano')).toBeInTheDocument();
    expect(within(list).getByText('Diario')).toBeInTheDocument();
    expect(
      screen.getByText("Imported 2 notes. foto.png isn't a Markdown file."),
    ).toBeInTheDocument();
    expect(screen.queryByText('Drop to import as notes')).toBeNull();
  });

  it('opens a single imported note', async () => {
    renderApp();
    await waitFor(() => expect(webview.handler).toBeDefined());
    act(() => webview.drag({ type: 'drop', paths: ['/n/Piano Q3.md'], position }));
    await waitFor(() => expect(commands('get_note')[0]?.args.id).toBe('note-1'));
    expect(await screen.findByText('Imported “Piano Q3”.')).toBeInTheDocument();
    // The note happened when its file was last modified.
    expect(await screen.findByText(/May 1, 2025/)).toBeInTheDocument();
  });

  it('⌘O asks for files and imports the ones chosen', async () => {
    core.chosen = ['/n/a.md', '/n/b.md'];
    renderApp();
    fireEvent.keyDown(window, { key: 'o', code: 'KeyO', metaKey: true });
    await waitFor(() => expect(commands('import_notes')).toHaveLength(1));
    expect(commands('choose_notes_to_import')[0]?.args).toEqual({
      title: 'Import Markdown files as notes',
      filter: 'Markdown files',
    });
    expect(await screen.findByText('Imported 2 notes.')).toBeInTheDocument();
  });

  it('does nothing when the open panel is cancelled', async () => {
    renderApp();
    fireEvent.keyDown(window, { key: 'o', code: 'KeyO', metaKey: true });
    await waitFor(() => expect(commands('choose_notes_to_import')).toHaveLength(1));
    await act(() => Promise.resolve());
    expect(commands('import_notes')).toHaveLength(0);
  });

  it('runs from the command bar and from the native menu', async () => {
    const user = renderApp();
    fireEvent.keyDown(window, { key: 'k', code: 'KeyK', metaKey: true });
    const option = await screen.findByRole('menuitem', { name: /Import notes…/ });
    expect(option).toHaveTextContent('⌘O');
    await user.click(option);
    await waitFor(() => expect(commands('choose_notes_to_import')).toHaveLength(1));

    act(() => core.handlers.get(MENU_COMMAND_EVENT)?.({ payload: 'importNotes' }));
    await waitFor(() => expect(commands('choose_notes_to_import')).toHaveLength(2));
  });

  it('says so when the import fails', async () => {
    core.failImport = true;
    renderApp();
    await waitFor(() => expect(webview.handler).toBeDefined());
    act(() => webview.drag({ type: 'drop', paths: ['/n/a.md'], position }));
    expect(
      await screen.findByText("The notes couldn't be imported (internal)."),
    ).toBeInTheDocument();
  });
});
