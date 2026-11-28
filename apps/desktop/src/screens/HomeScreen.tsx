import { isTauri } from '@tauri-apps/api/core';
import { motion, type Variants } from 'motion/react';
import { useEffect, useMemo, useState } from 'react';
import { Button, GridList, GridListItem } from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import type { Job } from '../jobs/useJobs';
import { duration, ease } from '../motion';
import { listSources, type SourceRow } from '../notes/api';
import { SHORTCUTS } from '../shell/shortcuts';
import { HouseIcon, NotePencilIcon } from '../ui/icons';
import { useSourceFormat } from './sources';

interface HomeScreenProps {
  jobs: readonly Job[];
  /** Changes when sources arrive from outside the screen (an import): read them again. */
  revision?: number;
  onOpenNote: (noteId: string) => void;
  onOpenMeeting: (meetingId: string) => void;
  onSources: () => void;
  onNewNote: () => void;
}

/** Rows in each tile. */
const TILE_ROWS = 6;

const list: Variants = { show: { transition: { staggerChildren: 0.05, delayChildren: 0.03 } } };
const rise: Variants = {
  hide: { opacity: 0, y: 10 },
  show: { opacity: 1, y: 0, transition: { duration: duration.d4, ease: ease.out } },
};

/** The job working right now, or the next one waiting. */
function activeJob(jobs: readonly Job[]): Job | undefined {
  return jobs.find((job) => job.state === 'running') ?? jobs.find((job) => job.state === 'queued');
}

/**
 * Home (phase 1): the latest meetings, what is being processed and the recent notes. The
 * tiles of the curator (doubts, changes, projects) join in phase 2.
 */
export function HomeScreen({
  jobs,
  revision = 0,
  onOpenNote,
  onOpenMeeting,
  onSources,
  onNewNote,
}: HomeScreenProps) {
  const { t, locale } = useI18n();
  const format = useSourceFormat();
  const [meetings, setMeetings] = useState<SourceRow[] | null>(() => (isTauri() ? null : []));
  const [notes, setNotes] = useState<SourceRow[] | null>(() => (isTauri() ? null : []));
  const today = useMemo(
    () =>
      new Intl.DateTimeFormat(locale, { weekday: 'long', day: 'numeric', month: 'long' }).format(
        new Date(),
      ),
    [locale],
  );

  // A job that starts or ends changes a meeting's status and title: read them again.
  const jobsKey = jobs.map((job) => `${job.id}:${job.state}`).join(',');

  useEffect(() => {
    if (!isTauri()) return;
    let active = true;
    Promise.all([
      listSources({ kind: 'meeting', limit: TILE_ROWS }),
      listSources({ kind: 'note', limit: TILE_ROWS }),
    ])
      .then(([latest, recent]) => {
        if (!active) return;
        setMeetings(latest.sources);
        setNotes(recent.sources);
      })
      .catch((error: unknown) => {
        console.error('Cannot read the overview', error);
        if (active) {
          setMeetings([]);
          setNotes([]);
        }
      });
    return () => {
      active = false;
    };
  }, [jobsKey, revision]);

  const job = activeJob(jobs);
  const jobTarget = job && meetings?.find((row) => row.id === job.targetId);
  const empty = meetings?.length === 0 && notes?.length === 0;

  const status = (row: SourceRow) => {
    if (row.status === 'recording')
      return <span className="chip chip-live">{t('source-status-recording')}</span>;
    if (row.status === 'processing')
      return <span className="chip chip-busy">{t('source-status-processing')}</span>;
    if (row.status === 'failed')
      return <span className="chip chip-danger">{t('source-status-failed')}</span>;
    return null;
  };

  return (
    <motion.div className="screen" variants={list} initial="hide" animate="show">
      <motion.header className="screen-top screen-top-row" variants={rise}>
        <div>
          <p className="kicker">{today}</p>
          <h1 className="screen-title">{t('home-title')}</h1>
        </div>
        {job && (
          <div className="home-proc" role="status">
            <span className="home-pulse" aria-hidden />
            {jobTarget && <span className="home-proc-name">{format.name(jobTarget)}</span>}
            <span>{job.state === 'queued' ? t('job-queued') : t(`job-${job.kind}`)}</span>
            {job.state === 'running' && job.progress !== null && (
              <>
                <span className="bar" aria-hidden>
                  <span className="bar-fill" style={{ transform: `scaleX(${job.progress})` }} />
                </span>
                <span className="home-pct">{Math.round(job.progress * 100)}%</span>
              </>
            )}
          </div>
        )}
      </motion.header>

      {empty ? (
        <motion.section className="empty" variants={rise} aria-label={t('home-empty-title')}>
          <span className="empty-ic" aria-hidden>
            <HouseIcon />
          </span>
          <h2 className="empty-title">{t('home-empty-title')}</h2>
          <p className="empty-body">{t('home-empty-body')}</p>
          <div className="home-empty-acts">
            <Button className="btn" onPress={onNewNote}>
              <NotePencilIcon aria-hidden />
              {t('note-new')}
              <kbd className="kbd" aria-hidden>
                {SHORTCUTS.newNote.label}
              </kbd>
            </Button>
          </div>
        </motion.section>
      ) : (
        <div className="home-grid">
          <motion.section
            className="mt-tile home-tile"
            variants={rise}
            aria-labelledby="home-meetings"
          >
            <h2 className="mt-th" id="home-meetings">
              <span>{t('home-latest-meetings')}</span>
              <Button className="home-link" onPress={onSources}>
                {t('home-all-sources')}
              </Button>
            </h2>
            {meetings !== null &&
              (meetings.length === 0 ? (
                <p className="home-none">{t('home-no-meetings')}</p>
              ) : (
                <GridList
                  className="home-list"
                  aria-labelledby="home-meetings"
                  items={meetings}
                  onAction={(key) => onOpenMeeting(String(key))}
                >
                  {(row) => (
                    <GridListItem
                      id={row.id}
                      className="home-row home-mrow"
                      textValue={format.name(row)}
                    >
                      <time className="home-time" dateTime={new Date(row.occurredAt).toISOString()}>
                        {format.when(row.occurredAt)}
                      </time>
                      <span className="home-name">{format.name(row)}</span>
                      <span className="home-end">{status(row)}</span>
                      <span className="home-len">{format.length(row)}</span>
                    </GridListItem>
                  )}
                </GridList>
              ))}
          </motion.section>

          <motion.section
            className="mt-tile home-tile"
            variants={rise}
            aria-labelledby="home-notes"
          >
            <h2 className="mt-th" id="home-notes">
              <span>{t('home-recent-notes')}</span>
              <Button className="home-link" onPress={onNewNote}>
                {t('note-new')}
              </Button>
            </h2>
            {notes !== null &&
              (notes.length === 0 ? (
                <p className="home-none">{t('home-no-notes')}</p>
              ) : (
                <GridList
                  className="home-list"
                  aria-labelledby="home-notes"
                  items={notes}
                  onAction={(key) => onOpenNote(String(key))}
                >
                  {(row) => (
                    <GridListItem
                      id={row.id}
                      className="home-row home-nrow"
                      textValue={format.name(row)}
                    >
                      <span className="home-name" data-untitled={!row.title && !row.preview}>
                        {format.name(row)}
                      </span>
                      <time className="home-time" dateTime={new Date(row.updatedAt).toISOString()}>
                        {format.when(row.updatedAt)}
                      </time>
                    </GridListItem>
                  )}
                </GridList>
              ))}
          </motion.section>
        </div>
      )}
    </motion.div>
  );
}
