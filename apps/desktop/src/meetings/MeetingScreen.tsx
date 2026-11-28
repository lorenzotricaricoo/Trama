import { isTauri } from '@tauri-apps/api/core';
import { motion, type Variants } from 'motion/react';
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Button } from 'react-aria-components';
import { useAppearance } from '../appearance';
import { useI18n } from '../i18n/I18nProvider';
import type { Job } from '../jobs/useJobs';
import { duration, ease } from '../motion';
import { recordingNotes, showRecordingPanel, type RecordingNote } from '../recording/api';
import { formatClock, minutes } from '../recording/format';
import { api as settingsApi, type Retention } from '../settings/api';
import { AudioPlayer, type PlayerHandle } from './AudioPlayer';
import {
  editSummary,
  getMeeting,
  regenerateSummary,
  retranscribe,
  retryJob,
  setSourceTitle,
  sourceSummary,
  type KeyPoint,
  type Meeting,
  type Summary,
} from './api';
import { meetingErrorMessage } from './errors';
import { MeetingTitle } from './MeetingTitle';
import { SummaryTile, Timestamp } from './SummaryTile';

interface MeetingScreenProps {
  meetingId: string;
  /** Opens at this moment, in milliseconds from the start: a search hit. */
  startAtMs?: number;
  jobs: readonly Job[];
  onNotice: (message: string) => void;
}

const list: Variants = { show: { transition: { staggerChildren: 0.045, delayChildren: 0.03 } } };
const rise: Variants = {
  hide: { opacity: 0, y: 10 },
  show: { opacity: 1, y: 0, transition: { duration: duration.d4, ease: ease.out } },
};

/** How long a line jumped to stays marked. */
const FLASH_MS = 1600;

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

/** The line being said at `ms`: the last one that started by then. */
function lineAt(transcript: readonly Line[], ms: number, kinds: Line['kind'][]): Line | undefined {
  let found: Line | undefined;
  for (const line of transcript) {
    if (line.atMs > ms) break;
    if (kinds.includes(line.kind)) found = line;
  }
  return found;
}

/** The summary as plain text, for the clipboard. */
function summaryText(title: string, summary: Summary): string {
  const points = summary.keyPoints.map(
    (point) => `- ${point.text}${point.atMs === null ? '' : ` (${formatClock(point.atMs)})`}`,
  );
  return [title, summary.body, points.join('\n')].filter(Boolean).join('\n\n');
}

const TranscriptLine = memo(function TranscriptLine({
  line,
  current,
  flash,
  onSeek,
  meLabel,
  othersLabel,
  noteLabel,
}: {
  line: Line;
  current: boolean;
  flash: boolean;
  /** Present while there is audio to move. */
  onSeek: ((ms: number) => void) | undefined;
  meLabel: string;
  othersLabel: string;
  noteLabel: string;
}) {
  return (
    <li
      className="mt-line"
      data-key={line.key}
      data-note={line.kind === 'note' || undefined}
      data-current={current || undefined}
      data-flash={flash || undefined}
    >
      {onSeek ? (
        <Timestamp ms={line.atMs} onJump={onSeek} />
      ) : (
        <span className="mt-ts">{formatClock(line.atMs)}</span>
      )}
      {line.kind === 'note' ? (
        <span className="mt-notew">{noteLabel}</span>
      ) : (
        <span className={line.role === 'me' ? 'mt-me' : 'mt-oth'}>
          {line.role === 'me' ? meLabel : othersLabel}
        </span>
      )}
      <span className="mt-text">{line.text}</span>
    </li>
  );
});

/**
 * The meeting page (flow F4): the title and summary the user can rewrite, the key points
 * whose timestamps lead to that moment, where the processing stands, the audio while it is
 * kept, and the transcript of the two tracks with the notes taken while recording.
 */
export default function MeetingScreen({
  meetingId,
  startAtMs,
  jobs,
  onNotice,
}: MeetingScreenProps) {
  const { t, locale } = useI18n();
  const { reduceMotion } = useAppearance();
  const [meeting, setMeeting] = useState<Meeting | null>(null);
  const [summary, setSummary] = useState<Summary | null>(null);
  const [notes, setNotes] = useState<RecordingNote[]>([]);
  const [missing, setMissing] = useState(false);
  const [retention, setRetention] = useState<Retention | null>(null);
  const [playingMs, setPlayingMs] = useState<number | null>(null);
  const [flashKey, setFlashKey] = useState<string | null>(null);
  const player = useRef<PlayerHandle>(null);
  const transcriptList = useRef<HTMLOListElement>(null);
  const flashTimer = useRef<number | undefined>(undefined);

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

  // Why the audio is gone: the retention setting, named in the transcript header.
  const audioDeleted = meeting?.audioDeleted ?? false;
  useEffect(() => {
    if (!audioDeleted) return;
    let active = true;
    settingsApi
      .librarySettings()
      .then((settings) => {
        if (active) setRetention(settings.audioRetention);
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, [audioDeleted]);

  useEffect(() => () => window.clearTimeout(flashTimer.current), []);

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

  const transcript = useMemo(() => (meeting ? lines(meeting, notes) : []), [meeting, notes]);
  const hasAudio = (meeting?.audio.length ?? 0) > 0;

  /** Moves the player and the transcript to a moment. */
  const jump = useCallback(
    (ms: number) => {
      player.current?.seek(ms);
      const line = lineAt(transcript, ms, ['segment', 'note']);
      if (!line) return;
      const element = transcriptList.current?.querySelector(`[data-key="${line.key}"]`);
      element?.scrollIntoView?.({ block: 'center', behavior: reduceMotion ? 'auto' : 'smooth' });
      setFlashKey(line.key);
      window.clearTimeout(flashTimer.current);
      flashTimer.current = window.setTimeout(() => setFlashKey(null), FLASH_MS);
    },
    [transcript, reduceMotion],
  );

  const seekOnly = useCallback((ms: number) => player.current?.seek(ms), []);

  // A search hit opens the meeting at its moment, once the transcript is there.
  const started = useRef(false);
  useEffect(() => {
    if (started.current || startAtMs === undefined || transcript.length === 0) return;
    started.current = true;
    jump(startAtMs);
  }, [startAtMs, transcript, jump]);

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
  const summarizing = mine.some(
    (job) => job.kind === 'summarize' && (job.state === 'running' || job.state === 'queued'),
  );
  const current = running ?? queued;
  const date = when.format(meeting.occurredAt);
  const placeholder = t('meeting-untitled', { date });
  const shownTitle = meeting.title || placeholder;

  const failedAction = (error: unknown, what: string, message: string) => {
    console.error(what, error);
    onNotice(t(message));
  };

  const retry = (job: Job) => {
    retryJob(job.id).catch((error: unknown) =>
      failedAction(error, 'Cannot retry the job', 'meeting-retry-failed'),
    );
  };

  const transcribeAgain = () => {
    retranscribe(meetingId).catch((error: unknown) =>
      failedAction(error, 'Cannot transcribe again', 'meeting-retry-failed'),
    );
  };

  const regenerate = () => {
    regenerateSummary(meetingId).catch((error: unknown) =>
      failedAction(error, 'Cannot generate the summary', 'meeting-retry-failed'),
    );
  };

  const rename = async (title: string) => {
    try {
      const renamed = await setSourceTitle(meetingId, title);
      setMeeting((found) => found && { ...found, title: renamed.title });
    } catch (error) {
      failedAction(error, 'Cannot rename the meeting', 'meeting-rename-failed');
    }
  };

  const saveSummary = async (body: string, keyPoints: KeyPoint[]) => {
    try {
      setSummary(await editSummary(meetingId, body, keyPoints));
    } catch (error) {
      failedAction(error, 'Cannot save the summary', 'meeting-summary-save-failed');
      throw error;
    }
  };

  const copySummary = (from: Summary) => {
    navigator.clipboard
      .writeText(summaryText(shownTitle, from))
      .then(() => onNotice(t('meeting-summary-copied')))
      .catch((error: unknown) =>
        failedAction(error, 'Cannot copy the summary', 'meeting-copy-failed'),
      );
  };

  // A failure the job can retry, or one the audio still on disk can recover from.
  const canTranscribeAgain =
    !failedTranscription &&
    meeting.errorCode !== 'audio_missing' &&
    !meeting.audioDeleted &&
    hasAudio;

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
            {canTranscribeAgain && (
              <Button className="btn" onPress={transcribeAgain}>
                {t('meeting-retry')}
              </Button>
            )}
          </div>
        );
      case 'ready':
        return null;
    }
  })();

  const language = (() => {
    if (!meeting.language) return null;
    try {
      return new Intl.DisplayNames([locale], { type: 'language' }).of(meeting.language) ?? null;
    } catch {
      return null;
    }
  })();
  const metas = [
    date,
    meeting.durationMs === null
      ? null
      : t('meeting-minutes', { count: minutes(meeting.durationMs) }),
    language,
  ].filter(Boolean);

  const audioNote =
    meeting.audioDeleted && meeting.status !== 'recording'
      ? retention
        ? t('meeting-audio-deleted', { setting: t(`settings-retention-${retention}`) })
        : t('meeting-audio-deleted-plain')
      : null;

  const currentKey =
    hasAudio && playingMs !== null && playingMs > 0
      ? lineAt(transcript, playingMs, ['segment'])?.key
      : undefined;

  return (
    <motion.div className="screen mt" variants={list} initial="hide" animate="show">
      <motion.header className="screen-top screen-top-row" variants={rise}>
        <div className="mt-head">
          <MeetingTitle title={meeting.title} placeholder={placeholder} onRename={rename} />
          <div className="mt-metas">
            <span className="mt-chip">{t('source-kind-meeting')}</span>
            <span className="mt-meta">{metas.join(' · ')}</span>
          </div>
        </div>
        {summary && (
          <Button className="btn" onPress={() => copySummary(summary)}>
            {t('meeting-copy-summary')}
          </Button>
        )}
      </motion.header>
      <div className="mt-body">
        {status && (
          <motion.section className="mt-tile" variants={rise}>
            {status}
          </motion.section>
        )}
        {hasAudio && (
          <motion.section className="mt-tile" variants={rise}>
            <AudioPlayer
              ref={player}
              tracks={meeting.audio}
              durationMs={meeting.durationMs}
              onTime={setPlayingMs}
            />
          </motion.section>
        )}
        {(summary || meeting.status === 'ready') && (
          <motion.section className="mt-tile" variants={rise} aria-labelledby="mt-summary">
            <SummaryTile
              summary={summary}
              failed={!!failedSummary}
              busy={summarizing}
              onGenerate={() => (failedSummary ? retry(failedSummary) : regenerate())}
              onSave={saveSummary}
              onJump={jump}
            />
          </motion.section>
        )}
        <motion.section className="mt-tile" variants={rise} aria-labelledby="mt-transcript">
          <h2 className="mt-th" id="mt-transcript">
            <span>{t('meeting-transcript')}</span>
            {audioNote && <span className="mt-meta">{audioNote}</span>}
          </h2>
          {transcript.length === 0 ? (
            <p className="mt-none">{t('meeting-transcript-pending')}</p>
          ) : (
            <ol className="mt-lines" ref={transcriptList}>
              {transcript.map((line) => (
                <TranscriptLine
                  key={line.key}
                  line={line}
                  current={line.key === currentKey}
                  flash={line.key === flashKey}
                  onSeek={hasAudio ? seekOnly : undefined}
                  meLabel={t('record-track-me')}
                  othersLabel={t('record-track-others')}
                  noteLabel={t('meeting-note')}
                />
              ))}
            </ol>
          )}
        </motion.section>
      </div>
    </motion.div>
  );
}
