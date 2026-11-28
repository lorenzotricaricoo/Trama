import { isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useEffect, useMemo, useState } from 'react';
import { Button, Label, ProgressBar } from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import { DownloadSimpleIcon, TrashIcon, WaveformIcon } from '../ui/icons';
import { api, errorCode, MODEL_EVENT, type LibrarySettings, type ModelStatus } from './api';
import { languageName } from './GeneralSection';
import type { UpdateLibrary } from './SettingsScreen';
import { ConfirmDialog, ErrorText, Group, SelectField } from './ui';

/**
 * Languages the transcription model handles (Parakeet v3: 25 European languages), offered
 * as the meeting language. Names come from Intl, in the interface language.
 */
export const MEETING_LANGUAGES = [
  'bg',
  'cs',
  'da',
  'de',
  'el',
  'en',
  'es',
  'et',
  'fi',
  'fr',
  'hr',
  'hu',
  'it',
  'lt',
  'lv',
  'mt',
  'nl',
  'pl',
  'pt',
  'ro',
  'ru',
  'sk',
  'sl',
  'sv',
  'uk',
] as const;

function statusTone(state: ModelStatus['state']) {
  if (state === 'ready') return 'ok';
  if (state === 'corrupt') return 'warn';
  return undefined;
}

interface Props {
  library: LibrarySettings;
  onLibrary: UpdateLibrary;
}

/** The meeting languages as select options, named in the interface language. */
export function useMeetingLanguageOptions() {
  const { locale } = useI18n();
  return useMemo(
    () =>
      MEETING_LANGUAGES.map((tag) => ({ id: tag, label: languageName(tag, locale) })).sort((a, b) =>
        a.label.localeCompare(b.label, locale),
      ),
    [locale],
  );
}

/** Follows the transcription model: its state as the core reports it, live. */
export function useModelStatus() {
  const [status, setStatus] = useState<ModelStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    let updated = false;
    const unlisten = isTauri()
      ? listen<ModelStatus>(MODEL_EVENT, (event) => {
          updated = true;
          setStatus(event.payload);
        })
      : Promise.resolve(() => {});
    // Listen first, then load: an event that arrives meanwhile is newer.
    void unlisten
      .then(() => api.modelStatus())
      .then((initial) => {
        if (active && !updated) setStatus(initial);
      })
      .catch((err: unknown) => {
        if (active) setError(`model_${errorCode(err)}`);
      });
    return () => {
      active = false;
      void unlisten.then((stop) => stop());
    };
  }, []);

  return { status, setStatus, error, setError };
}

/** The transcription model: its state, download with progress, check and removal. */
export function ModelCard() {
  const { t, locale } = useI18n();
  const { status, setStatus, error, setError } = useModelStatus();
  const [confirmRemove, setConfirmRemove] = useState(false);

  const run = async (call: () => Promise<ModelStatus>) => {
    setError(null);
    try {
      setStatus(await call());
    } catch (err) {
      setError(`model_${errorCode(err)}`);
    }
  };

  const gigabytes = (bytes: number) =>
    new Intl.NumberFormat(locale, {
      style: 'unit',
      unit: 'gigabyte',
      maximumFractionDigits: 1,
    }).format(bytes / 1e9);

  const busy = status?.state === 'downloading' || status?.state === 'verifying';

  return (
    <>
      <div className="set-card set-engine" data-state={status?.state}>
        <div className="set-engine-head">
          <span className="set-engine-ic" aria-hidden>
            <WaveformIcon />
          </span>
          <div className="set-engine-text">
            <div className="set-engine-name">
              {t('settings-model-name')}
              <span className="set-mono set-engine-spec">
                {status ? t('settings-model-spec', { size: gigabytes(status.totalBytes) }) : ''}
              </span>
            </div>
            {status && (
              <div className="set-status" data-tone={statusTone(status.state)}>
                <span className="set-dot" aria-hidden />
                {t(`settings-model-state-${status.state}`)}
              </div>
            )}
          </div>
          {status && (
            <div className="set-engine-actions">
              {(status.state === 'missing' || status.state === 'corrupt') && (
                <Button
                  className="set-btn"
                  data-variant="primary"
                  onPress={() => void run(api.downloadModel)}
                >
                  <DownloadSimpleIcon aria-hidden />
                  {status.state === 'corrupt'
                    ? t('settings-model-download-again')
                    : status.downloadedBytes > 0
                      ? t('settings-model-resume')
                      : t('settings-model-download')}
                </Button>
              )}
              {busy && (
                <Button className="set-btn" onPress={() => void run(api.cancelModelDownload)}>
                  {status.state === 'verifying'
                    ? t('settings-model-cancel-check')
                    : t('settings-model-pause')}
                </Button>
              )}
              {status.state === 'ready' && (
                <Button className="set-btn" onPress={() => void run(api.verifyModel)}>
                  {t('settings-model-verify')}
                </Button>
              )}
              {(status.state !== 'missing' || status.downloadedBytes > 0) && !busy && (
                <Button className="set-btn" onPress={() => setConfirmRemove(true)}>
                  <TrashIcon aria-hidden />
                  {t('settings-model-remove')}
                </Button>
              )}
            </div>
          )}
        </div>
        {status && busy && (
          <ProgressBar
            className="set-progress"
            value={(status.progress ?? 0) * 100}
            valueLabel={
              status.state === 'downloading'
                ? t('settings-model-progress', {
                    done: gigabytes(status.downloadedBytes),
                    total: gigabytes(status.totalBytes),
                  })
                : undefined
            }
          >
            {({ percentage, valueText }) => (
              <>
                <div className="set-meter-head">
                  <Label className="set-row-desc">
                    {status.state === 'verifying'
                      ? t('settings-model-verifying')
                      : t('settings-model-downloading')}
                  </Label>
                  <span className="set-meter-value">{valueText}</span>
                </div>
                <div className="set-meter-track">
                  <div
                    className="set-meter-fill"
                    style={{ transform: `scaleX(${(percentage ?? 0) / 100})` }}
                  />
                </div>
              </>
            )}
          </ProgressBar>
        )}
        {status?.errorCode != null && !busy && <ErrorText code={`model_${status.errorCode}`} />}
        <ErrorText code={error} />
      </div>
      <ConfirmDialog
        isOpen={confirmRemove}
        title={t('settings-model-remove-title')}
        body={t('settings-model-remove-body')}
        confirm={t('settings-model-remove-confirm')}
        destructive
        onConfirm={() => {
          setConfirmRemove(false);
          void run(api.removeModel);
        }}
        onCancel={() => setConfirmRemove(false)}
      />
    </>
  );
}

/** What the model does: its languages, and that it runs on this Mac, offline. */
export function ModelFacts() {
  const { t } = useI18n();
  return (
    <div className="set-facts">
      {(['languages', 'local', 'offline'] as const).map((fact) => (
        <div key={fact} className="set-fact">
          <b>{t(`settings-model-fact-${fact}`)}</b>
          <span>{t(`settings-model-fact-${fact}-desc`)}</span>
        </div>
      ))}
    </div>
  );
}

export function TranscriptionSection({ library, onLibrary }: Props) {
  const { t } = useI18n();
  const [meetingError, setMeetingError] = useState<string | null>(null);
  const meetingOptions = useMeetingLanguageOptions();

  return (
    <>
      <Group title={t('settings-group-transcription')} description={t('settings-model-desc')}>
        <ModelCard />
        <ModelFacts />
        <p className="set-credit">{t('settings-model-credit')}</p>
      </Group>

      <Group
        title={t('settings-meeting-language')}
        description={t('settings-meeting-language-desc')}
      >
        <SelectField
          label={t('settings-meeting-language')}
          labelHidden
          options={meetingOptions}
          value={library.meetingLanguage}
          onChange={(meetingLanguage) => {
            setMeetingError(null);
            void onLibrary({ meetingLanguage }).then(setMeetingError);
          }}
        />
        <ErrorText code={meetingError} />
      </Group>
    </>
  );
}
