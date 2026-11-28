import { motion } from 'motion/react';
import { useDeferredValue, useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import { Button, ToggleButton, ToggleButtonGroup, Toolbar, type Key } from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import { duration, ease } from '../motion';
import { Hint } from '../ui/Hint';
import {
  CheckSquareIcon,
  CodeIcon,
  ColumnsIcon,
  EyeIcon,
  LinkIcon,
  ListBulletsIcon,
  ListNumbersIcon,
  NoteIcon,
  PencilSimpleLineIcon,
  QuotesIcon,
  TextBIcon,
  TextHIcon,
  TextItalicIcon,
  type Icon,
} from '../ui/icons';
import { createNote, getNote, saveNote, setNoteTitle } from './api';
import { Autosaver, type SaveStatus } from './autosave';
import { onQuitFlush } from './flush';
import { afterLeftScreens, trackLeftScreen } from './pending';
import { NoteEditor, type FormatAction, type NoteEditorHandle } from './NoteEditor';
import { NotePreview } from './NotePreview';

interface NoteScreenProps {
  /** The note to open; undefined for a new note, created when the first text is saved. */
  noteId: string | undefined;
  /** Navigation entry of this screen, to record the new note's id in it. */
  entry: number;
  onCreated: (entry: number, noteId: string) => void;
  /** Shows a short message, for a link that can't be opened. */
  onNotice: (message: string) => void;
}

type Loaded =
  | { state: 'loading' }
  | { state: 'ready'; id: string | undefined; title: string; body: string; occurredAt: number }
  | {
      state: 'failed';
    };

/** Write: the text alone; split: text and preview side by side; read: the preview alone. */
export type NoteMode = 'write' | 'split' | 'read';

const MODES: { id: NoteMode; label: string; icon: Icon }[] = [
  { id: 'write', label: 'note-mode-write', icon: PencilSimpleLineIcon },
  { id: 'split', label: 'note-mode-split', icon: ColumnsIcon },
  { id: 'read', label: 'note-mode-read', icon: EyeIcon },
];

const TOOLS: ({ action: FormatAction; label: string; icon: Icon; shortcut?: string } | null)[] = [
  { action: 'heading', label: 'note-format-heading', icon: TextHIcon },
  { action: 'bold', label: 'note-format-bold', icon: TextBIcon, shortcut: '⌘B' },
  { action: 'italic', label: 'note-format-italic', icon: TextItalicIcon, shortcut: '⌘I' },
  null,
  { action: 'bullets', label: 'note-format-bullets', icon: ListBulletsIcon },
  { action: 'numbers', label: 'note-format-numbers', icon: ListNumbersIcon },
  { action: 'todo', label: 'note-format-todo', icon: CheckSquareIcon },
  null,
  { action: 'quote', label: 'note-format-quote', icon: QuotesIcon },
  { action: 'code', label: 'note-format-code', icon: CodeIcon },
  { action: 'link', label: 'note-format-link', icon: LinkIcon },
];

/** What the autosaver writes: the title and the text together, so one save creates the note. */
interface Draft {
  title: string;
  body: string;
}

const encode = (draft: Draft) => JSON.stringify([draft.title, draft.body]);
const decode = (text: string): Draft => {
  const [title, body] = JSON.parse(text) as [string, string];
  return { title, body };
};

/** Words of a text, in any script; Markdown marks don't count. */
function countWords(text: string): number {
  return text.match(/[\p{L}\p{N}]+(?:['’][\p{L}\p{N}]+)*/gu)?.length ?? 0;
}

/**
 * The Note screen (flow F3): a title and a Markdown document that save on their own, shown
 * as text, as text beside its preview, or as the preview alone. Edits are written after a
 * short pause in the typing, and at once when the editor loses focus, the window is
 * hidden, the screen is left or the app quits.
 */
export default function NoteScreen({ noteId, entry, onCreated, onNotice }: NoteScreenProps) {
  const { t, locale } = useI18n();
  // What this screen opened with; a new note gets its id later without reloading.
  const [opened] = useState({ noteId, entry });
  const [loaded, setLoaded] = useState<Loaded>({ state: 'loading' });
  const [status, setStatus] = useState<SaveStatus>('idle');
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const [title, setTitle] = useState('');
  const [body, setBody] = useState('');
  // A new note opens ready to type; an existing one opens as it reads.
  const [mode, setMode] = useState<NoteMode>(noteId === undefined ? 'write' : 'read');
  const preview = useDeferredValue(body);
  const editor = useRef<NoteEditorHandle>(null);
  const saver = useRef<Autosaver | null>(null);
  const draft = useRef<Draft>({ title: '', body: '' });
  const latest = useRef({ entry, onCreated });
  useEffect(() => {
    latest.current = { entry, onCreated };
  });

  // Reads the note once the writes of screens left earlier are done: coming Back to a new
  // note whose creation was still running opens that note instead of creating another.
  useEffect(() => {
    let active = true;
    afterLeftScreens(opened.entry)
      .then(async (createdId): Promise<Loaded> => {
        const id = opened.noteId ?? createdId;
        if (id === undefined)
          return { state: 'ready', id, title: '', body: '', occurredAt: Date.now() };
        const note = await getNote(id);
        return {
          state: 'ready',
          id,
          title: note.title,
          body: note.body,
          occurredAt: note.occurredAt,
        };
      })
      .then((ready) => {
        if (!active) return;
        if (ready.state === 'ready') {
          draft.current = { title: ready.title, body: ready.body };
          setTitle(ready.title);
          setBody(ready.body);
          // Nothing to read yet: open ready to write.
          if (!ready.body.trim()) setMode('write');
        }
        setLoaded(ready);
      })
      .catch((error: unknown) => {
        console.error('Cannot open the note', error);
        if (active) setLoaded({ state: 'failed' });
      });
    return () => {
      active = false;
    };
  }, [opened]);

  // One autosaver for the life of the screen; leaving the screen writes what is pending.
  const ready = loaded.state === 'ready';
  const readyId = loaded.state === 'ready' ? loaded.id : undefined;
  useEffect(() => {
    if (!ready) return;
    let id = readyId;
    // What the core holds, to write only what changed.
    let stored: Draft = { ...draft.current };
    const autosaver = new Autosaver(
      async (text) => {
        const next = decode(text);
        if (id === undefined) {
          const note = await createNote(next.title, next.body);
          id = note.id;
          stored = next;
          latest.current.onCreated(opened.entry, note.id);
          return;
        }
        if (next.body !== stored.body) {
          await saveNote(id, next.body);
          stored = { ...stored, body: next.body };
        }
        if (next.title !== stored.title) {
          await setNoteTitle(id, next.title);
          stored = { ...stored, title: next.title };
        }
      },
      (next) => {
        setStatus(next);
        if (next === 'saved') setSavedAt(Date.now());
      },
    );
    saver.current = autosaver;
    const flush = () => autosaver.flush();
    const onHidden = () => {
      if (document.visibilityState === 'hidden') void flush();
    };
    const removeQuitFlush = onQuitFlush(flush);
    document.addEventListener('visibilitychange', onHidden);
    window.addEventListener('pagehide', flush);
    return () => {
      document.removeEventListener('visibilitychange', onHidden);
      window.removeEventListener('pagehide', flush);
      saver.current = null;
      const first = flush();
      // A screen opened later from the same entry waits for this attempt (not the retries).
      trackLeftScreen(
        opened.entry,
        first.then(() => id),
      );
      // A failed save stays registered for the quit, and keeps retrying, until it lands.
      void first.then(async (saved) => {
        if (!saved) await autosaver.whenClean();
        removeQuitFlush();
      });
    };
  }, [ready, readyId, opened]);

  const change = (next: Partial<Draft>) => {
    draft.current = { ...draft.current, ...next };
    saver.current?.change(encode(draft.current));
  };

  const created = useMemo(
    () =>
      loaded.state === 'ready'
        ? new Intl.DateTimeFormat(locale, {
            day: 'numeric',
            month: 'long',
            year: 'numeric',
            hour: '2-digit',
            minute: '2-digit',
          }).format(loaded.occurredAt)
        : '',
    [loaded, locale],
  );
  const words = useMemo(() => countWords(preview), [preview]);
  const statusText =
    status === 'saved' && savedAt !== null
      ? t('note-status-saved-at', {
          time: new Intl.DateTimeFormat(locale, { hour: '2-digit', minute: '2-digit' }).format(
            savedAt,
          ),
        })
      : t(STATUS_MESSAGE[status]);

  if (loaded.state === 'failed') {
    return (
      <div className="screen">
        <section className="empty" role="alert">
          <span className="empty-ic" aria-hidden>
            <NoteIcon />
          </span>
          <h1 className="empty-title">{t('note-load-failed-title')}</h1>
          <p className="empty-body">{t('note-load-failed-body')}</p>
        </section>
      </div>
    );
  }

  const onTitleKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== 'Enter' || event.nativeEvent.isComposing) return;
    event.preventDefault();
    if (mode === 'read') setMode('write');
    // After the editor pane is shown again.
    requestAnimationFrame(() => editor.current?.focus());
  };

  return (
    <motion.div
      className="screen note-screen"
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0, transition: { duration: duration.d4, ease: ease.out } }}
    >
      <header className="note-head">
        <h1 className="visually-hidden">{t('note-title')}</h1>
        <input
          className="note-title"
          value={title}
          placeholder={t('note-title-placeholder')}
          aria-label={t('note-title-label')}
          maxLength={200}
          spellCheck
          disabled={!ready}
          onChange={(event) => {
            setTitle(event.target.value);
            change({ title: event.target.value });
          }}
          onBlur={() => void saver.current?.flush()}
          onKeyDown={onTitleKey}
        />
        {/* Visible state only: announcing every save would interrupt VoiceOver while typing.
            A failure is announced, below. */}
        <span className="note-status" data-status={status}>
          {statusText}
        </span>
      </header>
      <div className="visually-hidden" role="alert">
        {status === 'error' ? t('note-status-error') : ''}
      </div>

      <div className="note-bar">
        <Toolbar
          className="note-tools"
          aria-label={t('note-format-label')}
          data-hidden={mode === 'read' || undefined}
        >
          {TOOLS.map((tool, index) =>
            tool === null ? (
              <span key={`sep-${index}`} className="note-tools-sep" aria-hidden />
            ) : (
              <Hint
                key={tool.action}
                label={t(tool.label)}
                shortcut={tool.shortcut}
                placement="bottom"
              >
                <Button
                  className="icon-btn"
                  aria-label={t(tool.label)}
                  isDisabled={!ready || mode === 'read'}
                  onPress={() => editor.current?.format(tool.action)}
                >
                  <tool.icon aria-hidden />
                </Button>
              </Hint>
            ),
          )}
        </Toolbar>
        <ToggleButtonGroup
          className="note-modes"
          aria-label={t('note-mode-label')}
          selectionMode="single"
          disallowEmptySelection
          selectedKeys={[mode]}
          onSelectionChange={(keys: Set<Key>) => {
            const [next] = [...keys];
            if (next !== undefined) setMode(next as NoteMode);
          }}
        >
          {MODES.map((item) => (
            <ToggleButton key={item.id} id={item.id} className="note-mode">
              <item.icon aria-hidden />
              {t(item.label)}
            </ToggleButton>
          ))}
        </ToggleButtonGroup>
      </div>

      <div className="note-panes" data-mode={mode}>
        {/* The editor stays mounted in every mode, so undo and the caret survive a switch. */}
        <div className="note-pane note-pane-edit" hidden={mode === 'read'}>
          {loaded.state === 'ready' && (
            <NoteEditor
              ref={editor}
              initialText={loaded.body}
              label={t('note-editor-label')}
              placeholder={t('note-placeholder')}
              autoFocus={opened.noteId === undefined}
              onChange={(text) => {
                setBody(text);
                change({ body: text });
              }}
              onBlur={() => void saver.current?.flush()}
            />
          )}
        </div>
        {mode !== 'write' && (
          <div
            className="note-pane note-pane-preview"
            onDoubleClick={() => {
              if (mode === 'read') setMode('write');
            }}
          >
            <NotePreview
              text={preview}
              empty={t('note-preview-empty')}
              onLinkFailed={() => onNotice(t('note-link-failed'))}
            />
          </div>
        )}
      </div>

      <footer className="note-foot">
        <span>{created}</span>
        <span>
          {words > 0
            ? t('note-stats', { count: words, minutes: Math.max(1, Math.round(words / 200)) })
            : t('note-stats-empty')}
        </span>
      </footer>
    </motion.div>
  );
}

const STATUS_MESSAGE: Record<SaveStatus, string> = {
  idle: 'note-status-idle',
  pending: 'note-status-saving',
  saving: 'note-status-saving',
  saved: 'note-status-saved',
  error: 'note-status-error',
};
