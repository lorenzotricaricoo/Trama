import { useState } from 'react';
import { Button } from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import { formatClock } from '../recording/format';
import { PlusIcon, XIcon } from '../ui/icons';
import { USER_SUMMARY_MODEL, type KeyPoint, type Summary } from './api';

interface SummaryTileProps {
  summary: Summary | null;
  /** The summary job failed: retry it instead of generating one. */
  failed: boolean;
  /** A summary job is queued or running. */
  busy: boolean;
  onGenerate: () => void;
  /** Stores the edit; rejects when it could not. */
  onSave: (body: string, keyPoints: KeyPoint[]) => Promise<void>;
  /** A timestamp was pressed. */
  onJump: (ms: number) => void;
}

interface Draft {
  body: string;
  /** Each point keeps the moment it was pinned to. */
  points: (KeyPoint & { key: number })[];
}

/** A pressable timestamp: it moves the player and the transcript to that moment. */
export function Timestamp({ ms, onJump }: { ms: number; onJump: (ms: number) => void }) {
  const { t } = useI18n();
  const time = formatClock(ms);
  return (
    <Button
      className="mt-ts mt-jump"
      aria-label={t('meeting-jump', { time })}
      onPress={() => onJump(ms)}
    >
      {time}
    </Button>
  );
}

/** The summary with its key points; the user can rewrite both. */
export function SummaryTile({
  summary,
  failed,
  busy,
  onGenerate,
  onSave,
  onJump,
}: SummaryTileProps) {
  const { t } = useI18n();
  const [draft, setDraft] = useState<Draft | null>(null);
  const [saving, setSaving] = useState(false);

  const edit = (from: Summary) => {
    setDraft({
      body: from.body,
      points: from.keyPoints.map((point, index) => ({ ...point, key: index })),
    });
  };

  const save = () => {
    if (!draft || saving) return;
    setSaving(true);
    onSave(
      draft.body,
      draft.points.map(({ text, atMs }) => ({ text, atMs })),
    )
      .then(() => setDraft(null))
      .catch(() => undefined)
      .finally(() => setSaving(false));
  };

  const setPoint = (key: number, text: string) =>
    setDraft(
      (current) =>
        current && {
          ...current,
          points: current.points.map((point) => (point.key === key ? { ...point, text } : point)),
        },
    );

  const removePoint = (key: number) =>
    setDraft(
      (current) => current && { ...current, points: current.points.filter((p) => p.key !== key) },
    );

  const addPoint = () =>
    setDraft(
      (current) =>
        current && {
          ...current,
          points: [
            ...current.points,
            { text: '', atMs: null, key: Math.max(-1, ...current.points.map((p) => p.key)) + 1 },
          ],
        },
    );

  const header = (
    <h2 className="mt-th" id="mt-summary">
      <span>{t('meeting-summary')}</span>
      {summary && !draft && (
        <span className="mt-th-end">
          <span className="mt-meta">
            {summary.model === USER_SUMMARY_MODEL
              ? t('meeting-summary-edited')
              : t('meeting-summary-generated')}
          </span>
          <Button className="btn mt-btn-sm" onPress={() => edit(summary)}>
            {t('meeting-edit')}
          </Button>
        </span>
      )}
    </h2>
  );

  if (!summary) {
    return (
      <>
        {header}
        <div className="mt-empty-row">
          <span>{failed ? t('meeting-summary-failed') : t('meeting-summary-none')}</span>
          <Button className="btn" isDisabled={busy} onPress={onGenerate}>
            {failed ? t('meeting-retry') : t('meeting-summary-generate')}
          </Button>
        </div>
      </>
    );
  }

  if (draft) {
    return (
      <>
        {header}
        <textarea
          className="mt-field mt-body-field"
          value={draft.body}
          aria-label={t('meeting-summary-body-label')}
          rows={5}
          onChange={(event) => setDraft({ ...draft, body: event.target.value })}
          autoFocus
        />
        <ul className="mt-kp-edit" aria-label={t('meeting-key-points')}>
          {draft.points.map((point, index) => (
            <li className="mt-kp" key={point.key}>
              <input
                className="mt-field"
                value={point.text}
                aria-label={t('meeting-key-point-label', { n: index + 1 })}
                onChange={(event) => setPoint(point.key, event.target.value)}
              />
              <span className="mt-kp-end">
                {point.atMs !== null && <span className="mt-ts">{formatClock(point.atMs)}</span>}
                <Button
                  className="icon-btn"
                  aria-label={t('meeting-key-point-remove')}
                  onPress={() => removePoint(point.key)}
                >
                  <XIcon aria-hidden />
                </Button>
              </span>
            </li>
          ))}
        </ul>
        <div className="mt-edit-acts">
          <Button className="btn mt-btn-sm mt-add" onPress={addPoint}>
            <PlusIcon aria-hidden />
            {t('meeting-key-point-add')}
          </Button>
          <span className="mt-grow" />
          <Button className="btn mt-btn-sm" isDisabled={saving} onPress={() => setDraft(null)}>
            {t('meeting-cancel')}
          </Button>
          <Button className="btn mt-btn-sm mt-btn-primary" isDisabled={saving} onPress={save}>
            {t('meeting-save')}
          </Button>
        </div>
      </>
    );
  }

  return (
    <>
      {header}
      {summary.body && <p className="mt-sum">{summary.body}</p>}
      {summary.keyPoints.length > 0 && (
        <ul className="mt-kps" aria-label={t('meeting-key-points')}>
          {summary.keyPoints.map((point, index) => (
            <li className="mt-kp" key={index}>
              <span>{point.text}</span>
              {point.atMs !== null && <Timestamp ms={point.atMs} onJump={onJump} />}
            </li>
          ))}
        </ul>
      )}
    </>
  );
}
