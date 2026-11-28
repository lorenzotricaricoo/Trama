import { isTauri } from '@tauri-apps/api/core';
import { motion, type Variants } from 'motion/react';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { Button } from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import type { Job } from '../jobs/useJobs';
import { duration, ease } from '../motion';
import { recordingNotes, showRecordingPanel, type RecordingNote } from '../recording/api';
import { formatClock, minutes } from '../recording/format';
import {
  getMeeting,
  regenerateSummary,
  retryJob,
  sourceSummary,
  type Meeting,
  type Summary,
} from './api';
import { meetingErrorMessage } from './errors';

interface MeetingScreenProps {
  meetingId: string;
  jobs: readonly Job[];
  onNotice: (message: string) => void;
}

const list: Variants = { show: { transition: { staggerChildren: 0.045, delayChildren: 0.03 } } };
const rise: Variants = {
  hide: { opacity: 0, y: 10 },
  show: { opacity: 1, y: 0, transition: { duration: duration.d4, ease: ease.out } },
};

/** A line of the transcript: what was said, or a note taken at that moment. */
type Line =
  | { kind: 'segment'; key: string; atMs: number; role: 'me' | 'others'; text: string }
  | { kind: 'note'; key: string; atMs: number; text: string };

function lines(meeting: Meeting, notes: readonly RecordingNote[]): Line[] {
  const said: Line[] = meeting.segments.map((segment) => ({
    kind: 'segment',
    key: `s-${segment.id}`,
    atMs: segment.startMs,
    role: segment.role,
    text: segment.text,
  }));
  const noted: Line[] = notes.map((note) => ({
    kind: 'note',
    key: `n-${note.id}`,
    atMs: note.atMs,
    text: note.text,
  }));
  // A note goes after what was being said when it was taken.
  return [...said, ...noted].sort((a, b) => a.atMs - b.atMs || (a.kind === 'note' ? 1 : -1));
}

/**
 * The meeting page, kept minimal until the full one (#33): title, where the processing
 * stands, the summary with its key points, and the transcript with the notes taken while
 * recording.
 */
export default function MeetingScreen({ meetingId, jobs, onNotice }: MeetingScreenProps) {
  const { t, locale } = useI18n();
  const [meeting, setMeeting] = useState<Meeting | null>(null);
  const [summary, setSummary] = useState<Summary | null>(null);
  const [notes, setNotes] = useState<RecordingNote[]>([]);
  const [missing, setMissing] = useState(false);

  const load = useCallback(async () => {
    const [found, latest, taken] = await Promise.all([
      getMeeting(meetingId),
      sourceSummary(meetingId),
      recordingNotes(meetingId),
    ]);
    return { found, latest, taken };
  }, [meetingId]);

  const mine = jobs.filter((job) => job.targetId === meetingId);
  // A job that starts, ends or fails changes what the page shows: read it again.
  const jobsKey = mine.map((job) => `${job.id}:${job.state}`).join(',');

  useEffect(() => {
    if (!isTauri()) return;
    let active = true;
    load()
      .then(({ found, latest, taken }) => {
        if (!active) return;
        setMeeting(found);
        setSummary(latest);
        setNotes(taken);
      })
      .catch((error: unknown) => {
        console.error('Cannot read the meeting', error);
        if (active) setMissing(true);
      });
    return () => {
      active = false;
    };
  }, [load, jobsKey]);

  const when = useMemo(
    () =>
      new Intl.DateTimeFormat(locale, {
        day: 'numeric',
        month: 'short',
        hour: '2-digit',
        minute: '2-digit',
      }),
    [locale],
  );

  if (missing) {
    return (
      <div className="screen">
        <header className="screen-top">
          <h1 className="screen-title">{t('meeting-missing')}</h1>
        </header>
      </div>
    );
  }
  if (!meeting) return <div className="screen" aria-busy />;

  const running = mine.find((job) => job.state === 'running');
  const queued = mine.find((job) => job.state === 'queued');
  const failedTranscription = mine.find(
    (job) => job.kind === 'transcribe' && job.state === 'failed',
  );
  const failedSummary = mine.find((job) => job.kind === 'summarize' && job.state === 'failed');
  const current = running ?? queued;
  const title = meeting.title || t('meeting-untitled', { date: when.format(meeting.occurredAt) });

  const retry = (job: Job) => {
    retryJob(job.id).catch((error: unknown) => {
      console.error('Cannot retry the job', error);
      onNotice(t('meeting-retry-failed'));
    });
  };

  const regenerate = () => {
    regenerateSummary(meetingId).catch((error: unknown) => {
      console.error('Cannot generate the summary', error);
      onNotice(t('meeting-retry-failed'));
    });
  };

  const status = (() => {
    switch (meeting.status) {
      case 'recording':
        return (
          <div className="mt-status">
            <span className="mt-live" aria-hidden />
            <span className="mt-status-text">{t('meeting-recording')}</span>
            <Button
              className="btn"
              onPress={() => {
                showRecordingPanel().catch((error: unknown) => {
                  console.error('Cannot show the recording panel', error);
                });
              }}
            >
              {t('meeting-show-panel')}
            </Button>
          </div>
        );
      case 'processing': {
        const phase =
          current?.kind === 'summarize'
            ? t('meeting-summarizing')
            : current?.state === 'running'
              ? t('meeting-transcribing')
              : t('meeting-queued');
        const progress = current?.state === 'running' ? current.progress : null;
        return (
          <div className="mt-status">
            <span className="mt-status-text">{phase}</span>
            <span
              className="mt-bar"
              role="progressbar"
              aria-label={phase}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={progress === null ? undefined : Math.round(progress * 100)}
              data-indeterminate={progress === null || undefined}
            >
              <span
                className="mt-fill"
                style={{ width: `${Math.round((progress ?? 0.3) * 100)}%` }}
              />
            </span>
            {progress !== null && <span className="mt-pct">{Math.round(progress * 100)}%</span>}
          </div>
        );
      }
      case 'failed':
        return (
          <div className="mt-status" data-failed>
            <span className="mt-status-text" role="alert">
              {t(meetingErrorMessage(meeting.errorCode), { code: meeting.errorCode ?? '' })}
            </span>
            {failedTranscription && (
              <Button className="btn" onPress={() => retry(failedTranscription)}>
                {t('meeting-retry')}
              </Button>
            )}
          </div>
        );
      case 'ready':
        return null;
    }
  })();

  const transcript = lines(meeting, notes);

  return (
    <motion.div className="screen mt" variants={list} initial="hide" animate="show">
      <motion.header className="screen-top" variants={rise}>
        <h1 className="screen-title mt-title" data-untitled={!meeting.title || undefined}>
          {title}
        </h1>
        <div className="mt-metas">
          <span className="mt-chip">{t('source-kind-meeting')}</span>
          <span className="mt-meta">
            {when.format(meeting.occurredAt)}
            {meeting.durationMs !== null &&
              ` · ${t('meeting-minutes', { count: minutes(meeting.durationMs) })}`}
          </span>
        </div>
      </motion.header>
      <div className="mt-body">
        {status && (
          <motion.section className="mt-tile" variants={rise}>
            {status}
          </motion.section>
        )}
        {(summary || meeting.status === 'ready') && (
          <motion.section className="mt-tile" variants={rise} aria-labelledby="mt-summary">
            <h2 className="mt-th" id="mt-summary">
              {t('meeting-summary')}
            </h2>
            {summary ? (
              <>
                <p className="mt-sum">{summary.body}</p>
                {summary.keyPoints.map((point, index) => (
                  <div className="mt-kp" key={index}>
                    <span>{point.text}</span>
                    {point.atMs !== null && (
                      <span className="mt-ts">{formatClock(point.atMs)}</span>
                    )}
                  </div>
                ))}
              </>
            ) : (
              <div className="mt-empty-row">
                <span>
                  {failedSummary ? t('meeting-summary-failed') : t('meeting-summary-none')}
                </span>
                <Button
                  className="btn"
                  onPress={() => (failedSummary ? retry(failedSummary) : regenerate())}
                >
                  {failedSummary ? t('meeting-retry') : t('meeting-summary-generate')}
                </Button>
              </div>
            )}
          </motion.section>
        )}
        <motion.section className="mt-tile" variants={rise} aria-labelledby="mt-transcript">
          <h2 className="mt-th" id="mt-transcript">
            {t('meeting-transcript')}
          </h2>
          {transcript.length === 0 ? (
            <p className="mt-none">{t('meeting-transcript-pending')}</p>
          ) : (
            <ol className="mt-lines">
              {transcript.map((line) => (
                <li
                  key={line.key}
                  className="mt-line"
                  data-note={line.kind === 'note' || undefined}
                >
                  <span className="mt-ts">{formatClock(line.atMs)}</span>
                  {line.kind === 'note' ? (
                    <span className="mt-notew">{t('meeting-note')}</span>
                  ) : (
                    <span className={line.role === 'me' ? 'mt-me' : 'mt-oth'}>
                      {line.role === 'me' ? t('record-track-me') : t('record-track-others')}
                    </span>
                  )}
                  <span className="mt-text">{line.text}</span>
                </li>
              ))}
            </ol>
          )}
        </motion.section>
      </div>
    </motion.div>
  );
}
