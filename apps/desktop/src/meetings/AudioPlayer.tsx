import { convertFileSrc } from '@tauri-apps/api/core';
import { useEffect, useImperativeHandle, useRef, useState, type Ref } from 'react';
import { Button } from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import { formatClock } from '../recording/format';
import { PauseIcon, PlayIcon } from '../ui/icons';
import type { AudioTrack } from './api';

/** What the page asks of the player: jump to a moment. */
export interface PlayerHandle {
  seek: (ms: number) => void;
}

interface AudioPlayerProps {
  ref: Ref<PlayerHandle>;
  tracks: readonly AudioTrack[];
  /** The meeting's length, until the files say theirs. */
  durationMs: number | null;
  /** The playing position, a few times a second. */
  onTime: (ms: number) => void;
}

/** Past this gap the second track jumps back in step with the first. */
const MAX_DRIFT_S = 0.12;

/**
 * Plays the two tracks of a meeting together (me and the others are recorded apart, on
 * the same clock): the first leads, the second follows it.
 */
export function AudioPlayer({ ref, tracks, durationMs, onTime }: AudioPlayerProps) {
  const { t } = useI18n();
  const elements = useRef<(HTMLAudioElement | null)[]>([]);
  const [playing, setPlaying] = useState(false);
  const [timeMs, setTimeMs] = useState(0);
  const [lengthMs, setLengthMs] = useState(durationMs ?? 0);
  const [failed, setFailed] = useState(false);

  const all = () => elements.current.filter((element): element is HTMLAudioElement => !!element);

  const seek = (ms: number) => {
    const seconds = Math.max(0, ms) / 1000;
    for (const element of all()) element.currentTime = seconds;
    setTimeMs(Math.max(0, ms));
    onTime(Math.max(0, ms));
  };

  useImperativeHandle(ref, () => ({ seek }));

  // Leaving the page stops the sound.
  useEffect(() => {
    const current = elements.current;
    return () => {
      for (const element of current) element?.pause();
    };
  }, []);

  const toggle = () => {
    const players = all();
    if (playing) {
      for (const element of players) element.pause();
      return;
    }
    const lead = players[0];
    if (!lead) return;
    for (const element of players) element.currentTime = lead.currentTime;
    Promise.all(players.map((element) => element.play())).catch((error: unknown) => {
      console.error('Cannot play the meeting audio', error);
      for (const element of players) element.pause();
      setFailed(true);
    });
  };

  const onLeadTime = () => {
    const [lead, ...followers] = all();
    if (!lead) return;
    for (const follower of followers) {
      if (Math.abs(follower.currentTime - lead.currentTime) > MAX_DRIFT_S) {
        follower.currentTime = lead.currentTime;
      }
    }
    const ms = Math.round(lead.currentTime * 1000);
    setTimeMs(ms);
    onTime(ms);
  };

  const onMetadata = (element: HTMLAudioElement) => {
    if (Number.isFinite(element.duration)) {
      setLengthMs((current) => Math.max(current, Math.round(element.duration * 1000)));
    }
  };

  return (
    <div className="mt-player" role="group" aria-label={t('meeting-player')}>
      {tracks.map((track, index) => (
        <audio
          key={track.path}
          ref={(element) => {
            elements.current[index] = element;
          }}
          src={convertFileSrc(track.path)}
          preload="metadata"
          onLoadedMetadata={(event) => onMetadata(event.currentTarget)}
          onError={() => setFailed(true)}
          {...(index === 0
            ? {
                onTimeUpdate: onLeadTime,
                onPlay: () => setPlaying(true),
                onPause: () => setPlaying(false),
                onEnded: () => {
                  for (const element of all()) element.pause();
                },
              }
            : {})}
        />
      ))}
      <Button
        className="mt-play"
        aria-label={playing ? t('meeting-pause') : t('meeting-play')}
        isDisabled={failed}
        onPress={toggle}
      >
        {playing ? <PauseIcon aria-hidden weight="fill" /> : <PlayIcon aria-hidden weight="fill" />}
      </Button>
      <span className="mt-clock">{`${formatClock(timeMs)} / ${formatClock(lengthMs)}`}</span>
      <input
        className="mt-scrub"
        type="range"
        min={0}
        max={Math.max(lengthMs, 1)}
        step={100}
        value={Math.min(timeMs, lengthMs)}
        aria-label={t('meeting-position')}
        aria-valuetext={formatClock(timeMs)}
        disabled={failed}
        onChange={(event) => seek(Number(event.target.value))}
      />
      {failed && (
        <span className="mt-player-error" role="alert">
          {t('meeting-audio-failed')}
        </span>
      )}
    </div>
  );
}
