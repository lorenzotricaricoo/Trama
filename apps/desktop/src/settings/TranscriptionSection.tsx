import { isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useEffect, useState } from 'react';
import { Button, Label, ProgressBar } from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import { CheckIcon, DownloadSimpleIcon, TrashIcon, WarningIcon } from '../ui/icons';
import { api, errorCode, MODEL_EVENT, type ModelStatus } from './api';
import { ConfirmDialog, ErrorText, Group, Row } from './ui';

/** The transcription model: its state, download, check and removal. */
export function TranscriptionSection() {
  const { t, locale } = useI18n();
  const [status, setStatus] = useState<ModelStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirmRemove, setConfirmRemove] = useState(false);

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
      <Group title={t('settings-group-transcription')} description={t('settings-model-desc')}>
        {status === null ? (
          <Row>
            <ErrorText code={error} />
          </Row>
        ) : (
          <>
            <Row
              label={t('settings-model-name')}
              description={t('settings-model-size', { size: gigabytes(status.totalBytes) })}
            >
              <span
                className="set-chip"
                data-tone={status.state === 'corrupt' ? 'warn' : undefined}
              >
                {status.state === 'ready' && <CheckIcon aria-hidden />}
                {status.state === 'corrupt' && <WarningIcon aria-hidden />}
                {t(`settings-model-state-${status.state}`)}
              </span>
            </Row>
            {busy && (
              <Row wide>
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
                        <Label className="set-row-label">
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
              </Row>
            )}
            {status.errorCode !== null && !busy && <ErrorText code={`model_${status.errorCode}`} />}
            <ErrorText code={error} />
            <div className="set-actions">
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
              {(status.state !== 'missing' || status.downloadedBytes > 0) && (
                <Button className="set-btn" onPress={() => setConfirmRemove(true)}>
                  <TrashIcon aria-hidden />
                  {t('settings-model-remove')}
                </Button>
              )}
            </div>
          </>
        )}
        <p className="set-credit">{t('settings-model-credit')}</p>
      </Group>
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
