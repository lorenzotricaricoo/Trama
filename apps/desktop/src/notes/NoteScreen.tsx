import { motion } from 'motion/react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useI18n } from '../i18n/I18nProvider';
import { duration, ease } from '../motion';
import { NoteIcon } from '../ui/icons';
import { createNote, getNote, saveNote } from './api';
import { Autosaver, type SaveStatus } from './autosave';
import { onQuitFlush } from './flush';
import { afterLeftScreens, trackLeftScreen } from './pending';
import { NoteEditor } from './NoteEditor';

interface NoteScreenProps {
  /** The note to open; undefined for a new note, created when the first text is saved. */
  noteId: string | undefined;
  /** Navigation entry of this screen, to record the new note's id in it. */
  entry: number;
  onCreated: (entry: number, noteId: string) => void;
}

type Loaded =
  | { state: 'loading' }
  | { state: 'ready'; id: string | undefined; body: string; createdAt: number }
  | {
      state: 'failed';
    };

const STATUS_MESSAGE: Record<SaveStatus, string> = {
  idle: 'note-status-idle',
  pending: 'note-status-saving',
  saving: 'note-status-saving',
  saved: 'note-status-saved',
  error: 'note-status-error',
};

/**
 * The Note screen (flow F3): a Markdown editor that saves on its own. Text is written after
 * a short pause in the typing, and at once when the editor loses focus, the window is
 * hidden, the screen is left or the app quits.
 */
export default function NoteScreen({ noteId, entry, onCreated }: NoteScreenProps) {
  const { t, locale } = useI18n();
  // What this screen opened with; a new note gets its id later without reloading.
  const [opened] = useState({ noteId, entry });
  const [loaded, setLoaded] = useState<Loaded>({ state: 'loading' });
  const [status, setStatus] = useState<SaveStatus>('idle');
  const saver = useRef<Autosaver | null>(null);
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
        if (id === undefined) return { state: 'ready', id, body: '', createdAt: Date.now() };
        const note = await getNote(id);
        return { state: 'ready', id, body: note.body, createdAt: note.createdAt };
      })
      .then((ready) => {
        if (active) setLoaded(ready);
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
    const autosaver = new Autosaver(async (text) => {
      if (id !== undefined) {
        await saveNote(id, text);
        return;
      }
      const note = await createNote(text);
      id = note.id;
      latest.current.onCreated(opened.entry, note.id);
    }, setStatus);
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

  const created = useMemo(
    () =>
      loaded.state === 'ready'
        ? new Intl.DateTimeFormat(locale, {
            weekday: 'long',
            day: 'numeric',
            month: 'long',
            hour: '2-digit',
            minute: '2-digit',
          }).format(loaded.createdAt)
        : '',
    [loaded, locale],
  );

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

  return (
    <motion.div
      className="screen note-screen"
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0, transition: { duration: duration.d4, ease: ease.out } }}
    >
      <header className="note-head">
        <h1 className="visually-hidden">{t('note-title')}</h1>
        <span className="kicker">{created}</span>
        {/* Visible state only: announcing every save would interrupt VoiceOver while typing.
            A failure is announced, below. */}
        <span className="note-status" data-status={status}>
          {t(STATUS_MESSAGE[status])}
        </span>
      </header>
      <div className="visually-hidden" role="alert">
        {status === 'error' ? t('note-status-error') : ''}
      </div>
      {loaded.state === 'ready' && (
        <NoteEditor
          initialText={loaded.body}
          label={t('note-editor-label')}
          placeholder={t('note-placeholder')}
          onChange={(text) => saver.current?.change(text)}
          onBlur={() => void saver.current?.flush()}
        />
      )}
    </motion.div>
  );
}
