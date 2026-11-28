import { isTauri } from '@tauri-apps/api/core';
import { motion, type Variants } from 'motion/react';
import { useEffect, useMemo, useState } from 'react';
import { Button, GridList, GridListItem } from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import { duration, ease } from '../motion';
import { listSources, type SourceRow } from '../notes/api';
import { SHORTCUTS } from '../shell/shortcuts';
import { NoteIcon, NotePencilIcon, StackIcon, WaveformIcon } from '../ui/icons';

interface SourcesScreenProps {
  onOpenNote: (noteId: string) => void;
  onOpenMeeting: (meetingId: string) => void;
  onNewNote: () => void;
}

const list: Variants = { show: { transition: { staggerChildren: 0.045, delayChildren: 0.03 } } };
const rise: Variants = {
  hide: { opacity: 0, y: 10 },
  show: { opacity: 1, y: 0, transition: { duration: duration.d4, ease: ease.out } },
};

/**
 * Sources, kept minimal until the full list with filters and search (#35): meetings and
 * notes, newest first, each opening its page, and a way to write a new note.
 */
export function SourcesScreen({ onOpenNote, onOpenMeeting, onNewNote }: SourcesScreenProps) {
  const { t, locale } = useI18n();
  // Outside the app (a plain browser) there is no core to ask.
  const [rows, setRows] = useState<SourceRow[] | null>(() => (isTauri() ? null : []));
  const date = useMemo(
    () => new Intl.DateTimeFormat(locale, { day: 'numeric', month: 'short', year: 'numeric' }),
    [locale],
  );

  useEffect(() => {
    if (!isTauri()) return;
    let active = true;
    // The first page is enough until the full list (#35) pages through the rest.
    listSources()
      .then((page) => {
        if (active) setRows(page.sources);
      })
      .catch((error: unknown) => {
        console.error('Cannot list the sources', error);
        if (active) setRows([]);
      });
    return () => {
      active = false;
    };
  }, []);

  const newNote = (
    <Button className="btn" onPress={onNewNote}>
      <NotePencilIcon aria-hidden />
      {t('note-new')}
      <kbd className="kbd" aria-hidden>
        {SHORTCUTS.newNote.label}
      </kbd>
    </Button>
  );

  return (
    <motion.div className="screen" variants={list} initial="hide" animate="show">
      <motion.header className="screen-top screen-top-row" variants={rise}>
        <div>
          <h1 className="screen-title">{t('nav-sources')}</h1>
          <p className="screen-desc">{t('sources-description')}</p>
        </div>
        {newNote}
      </motion.header>
      {rows !== null && rows.length === 0 && (
        <motion.section className="empty" variants={rise} aria-label={t('sources-empty-title')}>
          <span className="empty-ic" aria-hidden>
            <StackIcon />
          </span>
          <h2 className="empty-title">{t('sources-empty-title')}</h2>
          <p className="empty-body">{t('sources-empty-body')}</p>
        </motion.section>
      )}
      {rows !== null && rows.length > 0 && (
        <motion.div className="src" variants={rise}>
          <GridList
            className="src-list"
            aria-label={t('sources-list-label')}
            items={rows}
            onAction={(key) => {
              const row = rows.find((candidate) => candidate.id === key);
              if (row?.kind === 'meeting') onOpenMeeting(row.id);
              else if (row) onOpenNote(row.id);
            }}
          >
            {(row) => {
              const meeting = row.kind === 'meeting';
              const name =
                row.title ||
                row.preview ||
                (meeting
                  ? t('meeting-untitled', { date: date.format(row.occurredAt) })
                  : t('note-untitled'));
              const Icon = meeting ? WaveformIcon : NoteIcon;
              return (
                <GridListItem id={row.id} className="src-row" textValue={name}>
                  <Icon className="src-ic" aria-hidden />
                  <span className="src-name" data-untitled={!row.title && !row.preview}>
                    {name}
                  </span>
                  <span className="src-kind">
                    {meeting ? t('source-kind-meeting') : t('source-kind-note')}
                  </span>
                  <time className="src-date" dateTime={new Date(row.occurredAt).toISOString()}>
                    {date.format(row.occurredAt)}
                  </time>
                </GridListItem>
              );
            }}
          </GridList>
        </motion.div>
      )}
    </motion.div>
  );
}
