import { useCallback, useMemo } from 'react';
import { useI18n } from '../i18n/I18nProvider';
import type { SourceRow } from '../notes/api';

/** Marks around a match in a search excerpt (see `HitPayload` in `src-tauri/src/notes.rs`). */
const HIGHLIGHT_START = '\u0002';
const HIGHLIGHT_END = '\u0003';

/** A search excerpt with its matches marked. */
export function Highlighted({ text }: { text: string }) {
  const parts = text.split(HIGHLIGHT_START);
  return (
    <>
      {parts.map((part, index) => {
        if (index === 0) return part;
        const end = part.indexOf(HIGHLIGHT_END);
        if (end < 0) return part;
        return (
          <span key={index}>
            <mark className="hit-mark">{part.slice(0, end)}</mark>
            {part.slice(end + 1)}
          </span>
        );
      })}
    </>
  );
}

/** The excerpt as plain text, for labels read aloud. */
export const plainExcerpt = (text: string) =>
  text.replaceAll(HIGHLIGHT_START, '').replaceAll(HIGHLIGHT_END, '');

const DAY_MS = 24 * 60 * 60 * 1000;

/**
 * Names and dates of sources as lists show them: the title, else a note's first line, else
 * "Meeting of …" or "Untitled note"; today's time, yesterday, the weekday within a week,
 * or the date.
 */
export function useSourceFormat() {
  const { t, locale } = useI18n();
  const formats = useMemo(
    () => ({
      date: new Intl.DateTimeFormat(locale, { day: 'numeric', month: 'short', year: 'numeric' }),
      dayMonth: new Intl.DateTimeFormat(locale, { day: 'numeric', month: 'short' }),
      time: new Intl.DateTimeFormat(locale, { hour: '2-digit', minute: '2-digit' }),
      weekday: new Intl.DateTimeFormat(locale, { weekday: 'short' }),
      relative: new Intl.RelativeTimeFormat(locale, { numeric: 'auto' }),
    }),
    [locale],
  );

  const name = useCallback(
    (row: SourceRow) =>
      row.title ||
      row.preview ||
      (row.kind === 'meeting'
        ? t('meeting-untitled', { date: formats.date.format(row.occurredAt) })
        : t('note-untitled')),
    [t, formats],
  );

  const when = useCallback(
    (at: number, now = Date.now()) => {
      const startOfToday = new Date(now).setHours(0, 0, 0, 0);
      if (at >= startOfToday) return formats.time.format(at);
      if (at >= startOfToday - DAY_MS) return formats.relative.format(-1, 'day');
      if (at >= startOfToday - 6 * DAY_MS) return formats.weekday.format(at);
      return new Date(at).getFullYear() === new Date(now).getFullYear()
        ? formats.dayMonth.format(at)
        : formats.date.format(at);
    },
    [formats],
  );

  /** "48 min", or nothing while the length is unknown. */
  const length = useCallback(
    (row: SourceRow) =>
      row.durationMs === null
        ? ''
        : t('source-duration', { minutes: Math.max(1, Math.round(row.durationMs / 60_000)) }),
    [t],
  );

  return { name, when, length, date: formats.date };
}
