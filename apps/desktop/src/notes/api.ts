import { invoke } from '@tauri-apps/api/core';

/** A note as the core sends it (see `src-tauri/src/notes.rs`). */
export interface Note {
  id: string;
  /** Empty until a summary or the user gives one. */
  title: string;
  /** Markdown text of the current version. */
  body: string;
  versionNumber: number;
  createdAt: number;
  updatedAt: number;
}

/** What a save stored. */
export interface Saved {
  versionNumber: number;
  savedAt: number;
}

/** A row of the Sources list. */
export interface SourceRow {
  id: string;
  kind: 'meeting' | 'note';
  title: string;
  occurredAt: number;
  updatedAt: number;
  /** For a note: its first line of text, shown while it has no title. */
  preview: string | null;
}

/** Where a page of sources ends: pass it as `before` to get the next page. */
export interface SourceCursor {
  occurredAt: number;
  id: string;
}

/** A page of the Sources list. */
export interface SourcePage {
  sources: SourceRow[];
  /** The next page's cursor; null on the last page. */
  next: SourceCursor | null;
}

/** Creates a note with the text typed so far. */
export const createNote = (body: string) => invoke<Note>('create_note', { body });

/** Saves the text of a note; resolves once the core has committed it. */
export const saveNote = (id: string, body: string) => invoke<Saved>('save_note', { id, body });

/** A note with the text of its current version. */
export const getNote = (id: string) => invoke<Note>('get_note', { id });

/** A page of sources not in the trash, newest first, optionally of one kind. */
export const listSources = (
  options: { kind?: SourceRow['kind']; before?: SourceCursor; limit?: number } = {},
) =>
  invoke<SourcePage>('list_sources', {
    kind: options.kind ?? null,
    before: options.before ?? null,
    limit: options.limit ?? null,
  });
