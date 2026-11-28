import { isTauri } from '@tauri-apps/api/core';
import { motion, type Variants } from 'motion/react';
import { useEffect, useMemo, useState } from 'react';
import {
  Button,
  GridList,
  GridListItem,
  Input,
  SearchField,
  ToggleButton,
  ToggleButtonGroup,
  type Key,
} from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import type { Job } from '../jobs/useJobs';
import { duration, ease } from '../motion';
import {
  listSources,
  searchSources,
  sourceCounts,
  type SearchResult,
  type SourceCounts,
  type SourceCursor,
  type SourceRow,
} from '../notes/api';
import { formatClock } from '../recording/format';
import { SHORTCUTS } from '../shell/shortcuts';
import {
  MagnifyingGlassIcon,
  NoteIcon,
  NotePencilIcon,
  StackIcon,
  WaveformIcon,
  XIcon,
} from '../ui/icons';
import { Highlighted, plainExcerpt, useSourceFormat } from './sources';

interface SourcesScreenProps {
  jobs: readonly Job[];
  onOpenNote: (noteId: string) => void;
  /** Opens a meeting, at a moment when a search hit points to one. */
  onOpenMeeting: (meetingId: string, atMs?: number) => void;
  onNewNote: () => void;
}

type Filter = 'all' | SourceRow['kind'];

/** Sources read per page of the list. */
const PAGE = 100;
/** Pause after typing before searching. */
const SEARCH_DELAY_MS = 180;

const list: Variants = { show: { transition: { staggerChildren: 0.045, delayChildren: 0.03 } } };
const rise: Variants = {
  hide: { opacity: 0, y: 10 },
  show: { opacity: 1, y: 0, transition: { duration: duration.d4, ease: ease.out } },
};

/** The progress of the job working on a meeting, from 0 to 1, when one is. */
function progressOf(jobs: readonly Job[], id: string): number | null {
  const job = jobs.find(
    (candidate) =>
      candidate.targetId === id && (candidate.state === 'running' || candidate.state === 'queued'),
  );
  return job ? (job.progress ?? 0) : null;
}

/** A meeting's state, when it is not simply ready. */
function StatusChip({ row }: { row: SourceRow }) {
  const { t } = useI18n();
  if (row.status === 'recording')
    return <span className="chip chip-live">{t('source-status-recording')}</span>;
  if (row.status === 'processing')
    return <span className="chip chip-busy">{t('source-status-processing')}</span>;
  if (row.status === 'failed')
    return <span className="chip chip-danger">{t('source-status-failed')}</span>;
  return null;
}

/**
 * Sources (flow F4): meetings and notes, newest first, with a filter by kind and a full-text
 * search. A search hit in a meeting opens it at the minute the words were said.
 */
export function SourcesScreen({ jobs, onOpenNote, onOpenMeeting, onNewNote }: SourcesScreenProps) {
  const { t } = useI18n();
  const format = useSourceFormat();
  const [filter, setFilter] = useState<Filter>('all');
  const [query, setQuery] = useState('');
  const [counts, setCounts] = useState<SourceCounts | null>(null);
  // Outside the app (a plain browser) there is no core to ask.
  const [rows, setRows] = useState<SourceRow[] | null>(() => (isTauri() ? null : []));
  const [next, setNext] = useState<SourceCursor | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  // The last search done, with what it searched for; kept while the next one runs.
  const [found, setFound] = useState<{ words: string; results: SearchResult[] } | null>(null);
  const words = query.trim();
  // The list stays until the first results arrive, so typing does not blank the table.
  const searching = words.length > 0 && found !== null;
  const results = searching ? found.results : null;

  // A job that starts or ends changes a meeting's status: read the list again.
  const jobsKey = jobs.map((job) => `${job.id}:${job.state}`).join(',');
  const kind = filter === 'all' ? undefined : filter;

  useEffect(() => {
    if (!isTauri()) return;
    let active = true;
    Promise.all([listSources({ kind, limit: PAGE }), sourceCounts()])
      .then(([page, total]) => {
        if (!active) return;
        setRows(page.sources);
        setNext(page.next);
        setCounts(total);
      })
      .catch((error: unknown) => {
        console.error('Cannot list the sources', error);
        if (active) setRows([]);
      });
    return () => {
      active = false;
    };
  }, [kind, jobsKey]);

  // The search reads every kind: the filter and its counts work on the results.
  useEffect(() => {
    if (!words || !isTauri()) return;
    let active = true;
    const timer = window.setTimeout(() => {
      searchSources(words)
        .then((results) => {
          if (active) setFound({ words, results });
        })
        .catch((error: unknown) => {
          console.error('Cannot search the sources', error);
          if (active) setFound({ words, results: [] });
        });
    }, SEARCH_DELAY_MS);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [words, jobsKey]);

  const loadMore = () => {
    if (!next || loadingMore) return;
    setLoadingMore(true);
    listSources({ kind, before: next, limit: PAGE })
      .then((page) => {
        setRows((current) => [...(current ?? []), ...page.sources]);
        setNext(page.next);
      })
      .catch((error: unknown) => console.error('Cannot list more sources', error))
      .finally(() => setLoadingMore(false));
  };

  const shown = useMemo(
    () => (results ?? []).filter((result) => !kind || result.source.kind === kind),
    [results, kind],
  );

  const tally = useMemo<SourceCounts | null>(() => {
    if (!results) return counts;
    return {
      meetings: results.filter((r) => r.source.kind === 'meeting').length,
      notes: results.filter((r) => r.source.kind === 'note').length,
    };
  }, [counts, results]);

  const filters: { id: Filter; label: string; count: number | undefined }[] = [
    {
      id: 'all',
      label: t('sources-filter-all'),
      count: tally ? tally.meetings + tally.notes : undefined,
    },
    { id: 'meeting', label: t('sources-filter-meetings'), count: tally?.meetings },
    { id: 'note', label: t('sources-filter-notes'), count: tally?.notes },
  ];
  const visible = results ? shown.length : (rows?.length ?? null);
  const total = searching
    ? visible
    : tally &&
      (kind === 'meeting'
        ? tally.meetings
        : kind === 'note'
          ? tally.notes
          : tally.meetings + tally.notes);

  const open = (row: SourceRow, atMs?: number) => {
    if (row.kind === 'meeting') onOpenMeeting(row.id, atMs);
    else onOpenNote(row.id);
  };

  const library = rows !== null && counts !== null && counts.meetings + counts.notes === 0;

  return (
    <motion.div className="screen" variants={list} initial="hide" animate="show">
      <motion.header className="screen-top screen-top-row" variants={rise}>
        <div>
          <h1 className="screen-title">{t('nav-sources')}</h1>
          <p className="screen-desc">{t('sources-description')}</p>
        </div>
        <div className="src-actions">
          <SearchField
            className="src-find"
            aria-label={t('sources-search')}
            value={query}
            onChange={setQuery}
          >
            <MagnifyingGlassIcon className="src-find-ic" aria-hidden />
            <Input className="src-find-input" placeholder={t('sources-search')} />
            <Button className="src-find-clear" aria-label={t('sources-search-clear')}>
              <XIcon aria-hidden />
            </Button>
          </SearchField>
          <Button className="btn" onPress={onNewNote}>
            <NotePencilIcon aria-hidden />
            {t('note-new')}
            <kbd className="kbd" aria-hidden>
              {SHORTCUTS.newNote.label}
            </kbd>
          </Button>
        </div>
      </motion.header>

      {library ? (
        <motion.section className="empty" variants={rise} aria-label={t('sources-empty-title')}>
          <span className="empty-ic" aria-hidden>
            <StackIcon />
          </span>
          <h2 className="empty-title">{t('sources-empty-title')}</h2>
          <p className="empty-body">{t('sources-empty-body')}</p>
        </motion.section>
      ) : (
        <motion.div className="src" variants={rise}>
          <div className="src-frow">
            <ToggleButtonGroup
              className="seg"
              aria-label={t('sources-filter')}
              selectionMode="single"
              disallowEmptySelection
              selectedKeys={[filter]}
              onSelectionChange={(keys) => {
                const [picked] = [...keys] as Key[];
                if (picked) setFilter(picked as Filter);
              }}
            >
              {filters.map((option) => (
                <ToggleButton key={option.id} id={option.id} className="seg-item">
                  {option.label}
                  {option.count !== undefined && <span className="seg-count">{option.count}</span>}
                </ToggleButton>
              ))}
            </ToggleButtonGroup>
            {total !== null && total !== undefined && (
              <span className="src-meta" aria-live="polite">
                {searching
                  ? t('sources-results', { count: total })
                  : t('sources-count', { count: total })}
              </span>
            )}
          </div>

          <div className="src-table">
            <div className="src-head" aria-hidden>
              <span />
              <span>{t('sources-column-title')}</span>
              <span>{t('sources-column-when')}</span>
              <span className="src-r">{t('sources-column-length')}</span>
            </div>
            {results ? (
              <GridList
                className="src-list"
                aria-label={t('sources-results-label')}
                items={shown.map((result) => ({ ...result, id: result.source.id }))}
                onAction={(key) => {
                  const result = shown.find((candidate) => candidate.source.id === key);
                  if (!result) return;
                  const moment = result.hits.find((hit) => hit.atMs !== null)?.atMs ?? undefined;
                  open(result.source, moment);
                }}
                renderEmptyState={() => (
                  <div className="src-empty">
                    <span className="empty-ic" aria-hidden>
                      <MagnifyingGlassIcon />
                    </span>
                    <span className="empty-title">
                      {t('sources-no-results', { query: found?.words ?? words })}
                    </span>
                    <span>{t('sources-no-results-body')}</span>
                    <Button
                      className="btn"
                      onPress={() => {
                        setQuery('');
                        setFilter('all');
                      }}
                    >
                      {t('sources-reset')}
                    </Button>
                  </div>
                )}
              >
                {(result) => {
                  const row = result.source;
                  const meeting = row.kind === 'meeting';
                  const Icon = meeting ? WaveformIcon : NoteIcon;
                  const title = result.hits.find((hit) => hit.kind === 'title');
                  const passages = result.hits.filter((hit) => hit.kind !== 'title');
                  const name = format.name(row);
                  return (
                    <GridListItem id={row.id} className="src-row src-hit" textValue={name}>
                      <Icon className="src-ic" aria-hidden />
                      <span className="src-main">
                        <span className="src-name">
                          <span className="src-title">
                            {title ? <Highlighted text={title.excerpt} /> : name}
                          </span>
                          <StatusChip row={row} />
                        </span>
                        {passages.map((hit, index) => (
                          <span className="src-passage" key={index}>
                            {hit.atMs !== null && meeting ? (
                              <Button
                                className="src-at"
                                aria-label={t('sources-open-at', {
                                  time: formatClock(hit.atMs),
                                  text: plainExcerpt(hit.excerpt),
                                })}
                                onPress={() => open(row, hit.atMs ?? undefined)}
                              >
                                {formatClock(hit.atMs)}
                              </Button>
                            ) : null}
                            <span className="src-excerpt">
                              <Highlighted text={hit.excerpt} />
                            </span>
                          </span>
                        ))}
                      </span>
                      <time className="src-date" dateTime={new Date(row.occurredAt).toISOString()}>
                        {format.when(row.occurredAt)}
                      </time>
                      <span className="src-len">{format.length(row)}</span>
                    </GridListItem>
                  );
                }}
              </GridList>
            ) : (
              rows !== null && (
                <GridList
                  className="src-list"
                  aria-label={t('sources-list-label')}
                  items={rows}
                  onAction={(key) => {
                    const row = rows.find((candidate) => candidate.id === key);
                    if (row) open(row);
                  }}
                  renderEmptyState={() => (
                    <div className="src-empty">
                      <span className="empty-title">{t('sources-none-of-kind')}</span>
                    </div>
                  )}
                >
                  {(row) => {
                    const meeting = row.kind === 'meeting';
                    const Icon = meeting ? WaveformIcon : NoteIcon;
                    const name = format.name(row);
                    const progress = meeting ? progressOf(jobs, row.id) : null;
                    return (
                      <GridListItem id={row.id} className="src-row" textValue={name}>
                        <Icon className="src-ic" aria-hidden />
                        <span className="src-name" data-untitled={!row.title && !row.preview}>
                          <span className="src-title">{name}</span>
                          <StatusChip row={row} />
                        </span>
                        <time
                          className="src-date"
                          dateTime={new Date(row.occurredAt).toISOString()}
                        >
                          {format.when(row.occurredAt)}
                        </time>
                        <span className="src-len">{format.length(row)}</span>
                        {progress !== null && (
                          <span className="src-progress bar-live" aria-hidden>
                            <span
                              className="bar-fill"
                              style={{ transform: `scaleX(${Math.max(progress, 0.04)})` }}
                            />
                          </span>
                        )}
                      </GridListItem>
                    );
                  }}
                </GridList>
              )
            )}
          </div>
          {!searching && next && (
            <div className="src-more">
              <Button className="btn" isDisabled={loadingMore} onPress={loadMore}>
                {t('sources-more')}
              </Button>
            </div>
          )}
        </motion.div>
      )}
    </motion.div>
  );
}
