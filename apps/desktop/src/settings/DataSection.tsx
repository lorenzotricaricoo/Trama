import { useEffect, useState } from 'react';
import { Button } from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import { HardDrivesIcon, InfoIcon } from '../ui/icons';
import {
  api,
  errorCode,
  RETENTIONS,
  type AppSettings,
  type ChosenFolder,
  type HeldAudio,
  type LibrarySettings,
  type Retention,
  type RetentionImpact,
} from './api';
import type { UpdateLibrary } from './SettingsScreen';
import { ConfirmDialog, ErrorText, Group, Segmented } from './ui';

interface Props {
  app: AppSettings;
  onApp: (app: AppSettings) => void;
  library: LibrarySettings;
  onLibrary: UpdateLibrary;
}

/** A deletion of existing audio waiting for the user to confirm it. */
type PendingDeletion =
  | { kind: 'change'; retention: Retention; impact: RetentionImpact }
  | { kind: 'held'; impact: RetentionImpact };

const deletes = (impact: RetentionImpact) => impact.deleteNow + impact.deleteLater > 0;

/** Data and privacy: how long audio is kept, and where the library is. */
export function DataSection({ app, onApp, library, onLibrary }: Props) {
  const { t } = useI18n();
  const [retentionError, setRetentionError] = useState<string | null>(null);
  const [locationError, setLocationError] = useState<string | null>(null);
  const [chosen, setChosen] = useState<ChosenFolder | null>(null);
  const [pending, setPending] = useState<PendingDeletion | null>(null);
  const [held, setHeld] = useState<HeldAudio | null>(null);
  const location = app.library;
  const retention = library.audioRetention;

  // Audio recorded before retention existed: what the current setting would do to it.
  useEffect(() => {
    let active = true;
    api
      .heldAudio()
      .then((found) => {
        if (active) setHeld(found);
      })
      .catch((err: unknown) => {
        if (active) setRetentionError(errorCode(err));
      });
    return () => {
      active = false;
    };
  }, [retention]);

  // Choosing a setting that would delete audio the user already has asks first.
  const chooseRetention = async (next: Retention) => {
    setRetentionError(null);
    try {
      const impact = await api.audioRetentionImpact(next);
      if (deletes(impact)) {
        setPending({ kind: 'change', retention: next, impact });
        return;
      }
    } catch (err) {
      setRetentionError(errorCode(err));
      return;
    }
    setRetentionError(await onLibrary({ audioRetention: next }));
  };

  const confirmDeletion = async () => {
    const confirmed = pending;
    setPending(null);
    if (confirmed === null) return;
    if (confirmed.kind === 'change') {
      setRetentionError(
        await onLibrary({ audioRetention: confirmed.retention }, { confirmAudioDeletion: true }),
      );
      return;
    }
    try {
      setHeld(await api.applyRetentionToHeldAudio());
    } catch (err) {
      setRetentionError(errorCode(err));
    }
  };

  const choose = async () => {
    setLocationError(null);
    try {
      const folder = await api.chooseLibraryFolder();
      if (folder === null) return;
      if (folder.error !== null) setLocationError(folder.error);
      else setChosen(folder);
    } catch (err) {
      setLocationError(errorCode(err));
    }
  };

  const move = async (path: string | null) => {
    setChosen(null);
    setLocationError(null);
    try {
      onApp({ ...app, library: await api.setLibraryLocation(path) });
    } catch (err) {
      setLocationError(errorCode(err));
    }
  };

  const reveal = async () => {
    setLocationError(null);
    try {
      await api.revealLibrary();
    } catch (err) {
      setLocationError(errorCode(err));
    }
  };

  return (
    <>
      <Group title={t('settings-group-retention')} description={t('settings-retention-desc')}>
        <Segmented
          label={t('settings-retention')}
          labelHidden
          options={RETENTIONS.map((option) => ({
            id: option,
            label: t(`settings-retention-${option}`),
          }))}
          value={retention}
          onChange={(value) => void chooseRetention(value as Retention)}
        />
        <div className="set-callout">
          <InfoIcon aria-hidden />
          <p>
            {t(`settings-retention-${retention}-desc`)} {t('settings-retention-note')}
          </p>
        </div>
        {held && held.count > 0 && deletes(held.impact) && (
          <div className="set-warn set-held" role="status">
            <InfoIcon aria-hidden />
            <div>
              <p>{t('settings-retention-held', { count: held.count })}</p>
              <Button
                className="set-link"
                onPress={() => setPending({ kind: 'held', impact: held.impact })}
              >
                {t('settings-retention-held-apply')}
              </Button>
            </div>
          </div>
        )}
        <ErrorText code={retentionError} />
      </Group>

      <Group title={t('settings-group-leaves')} description={t('settings-leaves-desc')}>
        <dl className="set-table">
          {(['audio', 'sources', 'index'] as const).map((item) => (
            <div key={item} className="set-table-row">
              <dt>{t(`settings-leaves-${item}`)}</dt>
              <dd>{t(`settings-leaves-${item}-desc`)}</dd>
            </div>
          ))}
        </dl>
      </Group>

      <Group title={t('settings-group-library')} description={t('settings-library-desc')}>
        <div className="set-pathbox">
          <HardDrivesIcon aria-hidden />
          <span className="set-mono set-pathbox-path" title={location.current}>
            {location.current}
          </span>
          <Button className="set-btn" data-variant="quiet" onPress={() => void reveal()}>
            {t('settings-library-reveal')}
          </Button>
          <Button
            className="set-btn"
            isDisabled={location.overridden}
            onPress={() => void choose()}
          >
            {t('settings-library-change')}
          </Button>
        </div>
        {location.restartRequired && (
          <p className="set-warn" role="status">
            <InfoIcon aria-hidden />
            {t('settings-library-restart', { path: location.next })}
          </p>
        )}
        {location.overridden && <p className="set-row-desc">{t('settings-library-overridden')}</p>}
        {!location.overridden && location.next !== location.default && (
          <Button className="set-link" onPress={() => void move(null)}>
            {t('settings-library-default')}
          </Button>
        )}
        <ErrorText code={locationError} />
      </Group>

      <ConfirmDialog
        isOpen={pending !== null}
        title={t('settings-retention-confirm-title')}
        body={
          pending && (
            <>
              {pending.impact.deleteNow > 0 && (
                <p>{t('settings-retention-confirm-now', { count: pending.impact.deleteNow })}</p>
              )}
              {pending.impact.deleteLater > 0 && (
                <p>
                  {t('settings-retention-confirm-later', {
                    count: pending.impact.deleteLater,
                    days: pending.impact.days ?? 0,
                  })}
                </p>
              )}
              <p>{t('settings-retention-confirm-undo')}</p>
            </>
          )
        }
        confirm={t('settings-retention-confirm')}
        destructive
        onConfirm={() => void confirmDeletion()}
        onCancel={() => setPending(null)}
      />

      <ConfirmDialog
        isOpen={chosen !== null}
        title={t('settings-library-confirm-title')}
        body={
          <>
            <p className="set-path">{chosen?.path}</p>
            <p>{t('settings-library-confirm-body')}</p>
            {chosen?.warning && (
              <p className="set-warn">
                {t(`settings-warning-${chosen.warning.replace(/_/g, '-')}`)}
              </p>
            )}
          </>
        }
        confirm={t('settings-library-confirm')}
        onConfirm={() => void move(chosen?.path ?? null)}
        onCancel={() => setChosen(null)}
      />
    </>
  );
}
