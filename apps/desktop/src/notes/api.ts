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
  /** For a meeting: where it is in its life. */
  status: 'recording' | 'processing' | 'ready' | 'failed' | null;
  /** For a meeting: its length, once known. */
  durationMs: number | null;
}

/** Where a search found the words, in a source. */
export interface SearchHit {
  kind: 'title' | 'transcript' | 'note' | 'recordingNote';
  /** The text around the match; each match sits between U+0002 and U+0003. */
  excerpt: string;
  /** Where it is in a meeting, from its start. */
  atMs: number | null;
}

/** A source found by a search, with its best hits. */
export interface SearchResult {
  source: SourceRow;
  hits: SearchHit[];
}

/** How many sources there are of each kind. */
export interface SourceCounts {
  meetings: number;
  notes: number;
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

/** Creates a note with the title and text typed so far; an empty title is left to the summary. */
export const createNote = (title: string, body: string) =>
  invoke<Note>('create_note', { title, body });

/** Renames a note; an empty title goes back to the summary. Resolves to the title kept. */
export const setNoteTitle = (id: string, title: string) =>
  invoke<string>('set_note_title', { id, title });

/** Opens a web or mail link of the preview in the default app. */
export const openLink = (url: string) => invoke<void>('open_link', { url });

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

/** Sources containing every word typed, best first, optionally of one kind. */
export const searchSources = (
  query: string,
  options: { kind?: SourceRow['kind']; limit?: number } = {},
) =>
  invoke<SearchResult[]>('search_sources', {
    query,
    kind: options.kind ?? null,
    limit: options.limit ?? null,
  });

/** How many meetings and notes are not in the trash. */
export const sourceCounts = () => invoke<SourceCounts>('source_counts');
