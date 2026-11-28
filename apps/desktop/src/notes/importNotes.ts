import { invoke, isTauri } from '@tauri-apps/api/core';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { useCallback, useEffect, useRef, useState } from 'react';
import type { Translate } from '../i18n/catalogs';

/**
 * Largest file imported, in megabytes, and most files at once: the same limits as
 * `crates/core/src/import.rs` (a test in `src-tauri/src/notes.rs` checks they match).
 */
export const MAX_FILE_MB = 2;
export const MAX_FILES = 1000;

/** Why a file was not imported (see `SkipReason` in the core). */
export type SkipReason =
  'not_markdown' | 'not_text' | 'too_large' | 'unreadable' | 'duplicate' | 'too_many';

/** What an import did (see `import_notes` in `src-tauri/src/notes.rs`). */
export interface ImportResult {
  imported: { id: string; title: string }[];
  skipped: { name: string; reason: SkipReason }[];
}

/** Imports Markdown files, and the Markdown files inside folders, as notes. */
export const importNotes = (paths: string[]) => invoke<ImportResult>('import_notes', { paths });

/** Asks for Markdown files in the system's open panel; empty when cancelled. */
export const chooseNotesToImport = (title: string, filter: string) =>
  invoke<string[]>('choose_notes_to_import', { title, filter });

/** Reasons in the order the message lists them: what the user can act on first. */
const REASONS: readonly SkipReason[] = [
  'not_text',
  'too_large',
  'not_markdown',
  'unreadable',
  'duplicate',
  'too_many',
];

/** File names shown for each reason; the others are counted. */
const NAMES_SHOWN = 2;

/** The names of some files, the rest counted: “a.md, b.md and 3 more”. */
function fileList(t: Translate, names: string[]): string {
  const shown = names.slice(0, NAMES_SHOWN).join(', ');
  const more = names.length - NAMES_SHOWN;
  return more > 0 ? t('import-files-more', { names: shown, count: more }) : shown;
}

/** The notice after an import: how many notes arrived and why any file was left out. */
export function importMessage(t: Translate, result: ImportResult): string {
  const parts: string[] = [];
  const [first, ...others] = result.imported;
  if (!first) parts.push(t('import-none'));
  else if (others.length === 0) parts.push(t('import-done-one', { title: first.title }));
  else parts.push(t('import-done', { count: result.imported.length }));
  for (const reason of REASONS) {
    const names = result.skipped.filter((file) => file.reason === reason).map((file) => file.name);
    if (names.length === 0) continue;
    parts.push(
      t(`import-skip-${reason}`, {
        files: fileList(t, names),
        count: names.length,
        limit: `${MAX_FILE_MB} MB`,
        max: MAX_FILES,
      }),
    );
  }
  return parts.join(' ');
}

interface NoteImportOptions {
  t: Translate;
  /** Called after every import that ran, with what it did. */
  onImported: (result: ImportResult) => void;
  /** Called when the import itself failed (the library could not be written). */
  onFailed: (error: unknown) => void;
}

/**
 * Importing Markdown files as notes (#57): files dropped on the window, or chosen with
 * ⌘O. `dragging` is true while files are held over the window. One import runs at a time.
 */
export function useNoteImport({ t, onImported, onFailed }: NoteImportOptions) {
  const [dragging, setDragging] = useState(false);
  const [importing, setImporting] = useState(false);
  const busy = useRef(false);
  // The latest callbacks, so the drop listener is added once.
  const latest = useRef({ onImported, onFailed });
  useEffect(() => {
    latest.current = { onImported, onFailed };
  });

  const importPaths = useCallback((paths: string[]) => {
    if (paths.length === 0 || busy.current) return;
    busy.current = true;
    setImporting(true);
    importNotes(paths)
      .then((result) => latest.current.onImported(result))
      .catch((error: unknown) => latest.current.onFailed(error))
      .finally(() => {
        busy.current = false;
        setImporting(false);
      });
  }, []);

  const choose = useCallback(() => {
    if (!isTauri() || busy.current) return;
    chooseNotesToImport(t('import-dialog-title'), t('import-dialog-filter'))
      .then(importPaths)
      .catch((error: unknown) => latest.current.onFailed(error));
  }, [t, importPaths]);

  useEffect(() => {
    if (!isTauri()) return;
    const unlisten = getCurrentWebview().onDragDropEvent((event) => {
      const drag = event.payload;
      if (drag.type === 'enter') setDragging(drag.paths.length > 0);
      else if (drag.type === 'leave') setDragging(false);
      else if (drag.type === 'drop') {
        setDragging(false);
        importPaths(drag.paths);
      }
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [importPaths]);

  return { dragging, importing, choose, importPaths };
}
