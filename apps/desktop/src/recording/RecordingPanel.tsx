import { isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { AnimatePresence, MotionConfig, motion } from 'motion/react';
import { useEffect, useRef, useState, type KeyboardEvent } from 'react';
import { Button } from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import { duration, ease } from '../motion';
import { SHORTCUTS, useShortcuts } from '../shell/shortcuts';
import { ArrowsOutSimpleIcon } from '../ui/icons';
import { useThemeClasses } from '../useThemeClasses';
import {
  addRecordingNote,
  RECORDING_LEVEL_EVENT,
  RECORDING_NOTICE_EVENT,
  recordingNotes,
  setPanelCompact,
  stopRecording,
  type RecordingNote,
  type RecordingNotice,
  type TrackLevel,
  type TrackSource,
} from './api';
import { recordingNoticeMessage } from './errors';
import { formatClock, meterFill } from './format';
import { useElapsed, useRecordingState } from './useRecordingState';

const noop = () => undefined;

/** A level meter of one track, updated outside React at the pace of the levels. */
function Meter({
  label,
  meterRef,
}: {
  label: string;
  meterRef: (element: HTMLDivElement | null) => void;
}) {
  return (
    <div className="rec-lv">
      <span>{label}</span>
      <div className="rec-track" aria-hidden>
        <div className="rec-level" ref={meterRef} />
      </div>
    </div>
  );
}

/** The warnings shown, one per track; a track that starts sounding clears its own. */
type Warnings = Partial<Record<TrackSource, string>>;

/**
 * The floating recording panel (its own window, see `recording.rs`): the timer, the levels
 * of the two tracks, notes pinned to the current minute, and Stop. "Reduce" shrinks it to
 * a pill with the timer.
 */
export function RecordingPanel() {
  const { t } = useI18n();
  useThemeClasses();
  const { status, receivedAt } = useRecordingState();
  const elapsed = useElapsed(status, receivedAt);
  const [compact, setCompact] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [draft, setDraft] = useState('');
  const [notes, setNotes] = useState<RecordingNote[]>([]);
  const [warnings, setWarnings] = useState<Warnings>({});
  const [noteFailed, setNoteFailed] = useState(false);
  const meters = useRef<Partial<Record<TrackSource, HTMLDivElement | null>>>({});
  const list = useRef<HTMLOListElement>(null);
  const meetingId = status?.meetingId;

  // The panel shows above a transparent window: no page background.
  useEffect(() => {
    document.documentElement.classList.add('rec-window');
    return () => document.documentElement.classList.remove('rec-window');
  }, []);

  // Notes already taken, when the panel opens again during the recording.
  useEffect(() => {
    if (!meetingId) return;
    let active = true;
    recordingNotes(meetingId)
      .then((stored) => {
        if (active) setNotes(stored);
      })
      .catch((error: unknown) => {
        console.error('Cannot read the recording notes', error);
      });
    return () => {
      active = false;
    };
  }, [meetingId]);

  useEffect(() => {
    if (!isTauri()) return;
    const levels = listen<TrackLevel>(RECORDING_LEVEL_EVENT, (event) => {
      const meter = meters.current[event.payload.source];
      meter?.style.setProperty('--fill', String(meterFill(event.payload.rmsDb)));
    });
    const notices = listen<RecordingNotice>(RECORDING_NOTICE_EVENT, (event) => {
      const { source, kind, code } = event.payload;
      setWarnings((current) => {
        if (kind === 'started' || kind === 'device_changed') {
          const rest = { ...current };
          delete rest[source];
          return rest;
        }
        const message = recordingNoticeMessage(code);
        return message ? { ...current, [source]: message } : current;
      });
    });
    return () => {
      for (const unlisten of [levels, notices]) {
        void unlisten.then((stop) => {
          stop();
        });
      }
    };
  }, []);

  const stop = () => {
    if (stopping) return;
    setStopping(true);
    // The core closes this window once the files are finished.
    stopRecording().catch((error: unknown) => {
      console.error('Cannot stop the recording', error);
      setStopping(false);
    });
  };

  const resize = (next: boolean) => {
    setCompact(next);
    setPanelCompact(next).catch((error: unknown) => {
      console.error('Cannot resize the recording panel', error);
    });
  };

  useShortcuts(
    {
      record: stop,
      commandBar: noop,
      toggleSidebar: noop,
      settings: noop,
      back: noop,
      newNote: noop,
    },
    ['record'],
  );

  const addNote = () => {
    const text = draft.trim();
    if (!text) return;
    setDraft('');
    setNoteFailed(false);
    addRecordingNote(text)
      .then((note) => {
        setNotes((current) => [...current, note]);
        // The newest note in view.
        requestAnimationFrame(() => {
          const element = list.current;
          if (element) element.scrollTop = element.scrollHeight;
        });
      })
      .catch((error: unknown) => {
        console.error('Cannot save the note', error);
        // The text comes back, so nothing typed is lost.
        setDraft((current) => (current ? `${text} ${current}` : text));
        setNoteFailed(true);
      });
  };

  const onInputKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter' && !event.nativeEvent.isComposing) {
      event.preventDefault();
      addNote();
    }
  };

  const clock = formatClock(elapsed);
  const shownWarnings = Object.values(warnings);
  const fade = { duration: duration.d2, ease: ease.out };

  if (compact) {
    return (
      <MotionConfig reducedMotion="user">
        <div className="rec-pill" data-tauri-drag-region>
          <span className="rec-live" aria-hidden />
          <span className="rec-pill-clock" role="timer" data-tauri-drag-region>
            {clock}
          </span>
          {shownWarnings.length > 0 && (
            <span className="rec-pill-warn" role="status" title={t(shownWarnings[0] ?? '')}>
              !
            </span>
          )}
          <Button
            className="rec-pill-btn"
            aria-label={t('record-expand')}
            onPress={() => resize(false)}
          >
            <ArrowsOutSimpleIcon aria-hidden />
          </Button>
          <Button
            className="rec-pill-btn rec-pill-stop"
            aria-label={t('record-stop')}
            isDisabled={stopping}
            onPress={stop}
          >
            <span className="rec-stopsq" aria-hidden />
          </Button>
        </div>
      </MotionConfig>
    );
  }

  return (
    <MotionConfig reducedMotion="user">
      <section className="rec" aria-label={t('record-panel-label')} data-tauri-drag-region>
        <div className="rec-head" data-tauri-drag-region>
          <span className="rec-live" aria-hidden />
          <span data-tauri-drag-region>{t('record-in-progress')}</span>
          <span className="rec-stophint" data-tauri-drag-region>
            <kbd className="kbd">{SHORTCUTS.record.label}</kbd>
            {t('record-stop-hint')}
          </span>
        </div>
        <div className="rec-clock" role="timer" data-tauri-drag-region>
          {clock}
        </div>
        <Meter
          label={t('record-track-me')}
          meterRef={(element) => {
            meters.current.microphone = element;
          }}
        />
        <Meter
          label={t('record-track-others')}
          meterRef={(element) => {
            meters.current.system = element;
          }}
        />
        <AnimatePresence initial={false}>
          {shownWarnings.map((message) => (
            <motion.p
              key={message}
              className="rec-warn"
              role="status"
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: 'auto' }}
              exit={{ opacity: 0, height: 0 }}
              transition={fade}
            >
              {t(message)}
            </motion.p>
          ))}
        </AnimatePresence>
        <label className="rec-input" data-failed={noteFailed || undefined}>
          <input
            type="text"
            value={draft}
            placeholder={t('record-note-placeholder')}
            aria-label={t('record-note-label')}
            aria-describedby={noteFailed ? 'rec-note-error' : undefined}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={onInputKey}
            disabled={stopping}
            autoFocus
          />
          <kbd className="kbd" aria-hidden>
            ↵
          </kbd>
        </label>
        {noteFailed && (
          <p id="rec-note-error" className="rec-warn" role="alert">
            {t('record-note-failed')}
          </p>
        )}
        <ol className="rec-notes" ref={list} aria-label={t('record-notes-label')}>
          {notes.map((note) => (
            <motion.li
              key={note.id}
              className="rec-note"
              initial={{ opacity: 0, y: 6 }}
              animate={{ opacity: 1, y: 0 }}
              transition={fade}
            >
              <span className="rec-note-at">{formatClock(note.atMs)}</span>
              <span className="rec-note-text">{note.text}</span>
            </motion.li>
          ))}
        </ol>
        <div className="rec-acts">
          <Button className="btn" onPress={() => resize(true)}>
            {t('record-reduce')}
          </Button>
          <Button className="btn rec-stop" isDisabled={stopping} onPress={stop}>
            <span className="rec-stopsq" aria-hidden />
            {stopping ? t('record-stopping') : t('record-stop')}
          </Button>
        </div>
      </section>
    </MotionConfig>
  );
}
