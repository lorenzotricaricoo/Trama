import { useState } from 'react';
import { Button } from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import { HardDrivesIcon, InfoIcon } from '../ui/icons';
import {
  api,
  errorCode,
  RETENTIONS,
  type AppSettings,
  type ChosenFolder,
  type LibrarySettings,
  type Retention,
} from './api';
import type { UpdateLibrary } from './SettingsScreen';
import { ConfirmDialog, ErrorText, Group, Segmented } from './ui';

interface Props {
  app: AppSettings;
  onApp: (app: AppSettings) => void;
  library: LibrarySettings;
  onLibrary: UpdateLibrary;
}

/** Data and privacy: how long audio is kept, and where the library is. */
export function DataSection({ app, onApp, library, onLibrary }: Props) {
  const { t } = useI18n();
  const [retentionError, setRetentionError] = useState<string | null>(null);
  const [locationError, setLocationError] = useState<string | null>(null);
  const [chosen, setChosen] = useState<ChosenFolder | null>(null);
  const location = app.library;

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
          value={library.audioRetention}
          onChange={(value) => {
            setRetentionError(null);
            void onLibrary({ audioRetention: value as Retention }).then(setRetentionError);
          }}
        />
        <div className="set-callout">
          <InfoIcon aria-hidden />
          <p>
            {t(`settings-retention-${library.audioRetention}-desc`)} {t('settings-retention-note')}
          </p>
        </div>
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
