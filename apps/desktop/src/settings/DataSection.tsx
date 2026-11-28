import { useState } from 'react';
import { Button, Label, Radio, RadioGroup, Text } from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import { FolderOpenIcon, InfoIcon } from '../ui/icons';
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
import { ConfirmDialog, ErrorText, Group, Row } from './ui';

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
        <Row wide>
          <RadioGroup
            className="set-radios"
            value={library.audioRetention}
            onChange={(value) => {
              setRetentionError(null);
              void onLibrary({ audioRetention: value as Retention }).then(setRetentionError);
            }}
          >
            <Label className="set-row-label">{t('settings-retention')}</Label>
            <div className="set-choices">
              {RETENTIONS.map((option) => (
                <Radio key={option} value={option} className="set-choice">
                  <span className="set-choice-dot" aria-hidden />
                  <span className="set-choice-text">
                    <span className="set-row-label">{t(`settings-retention-${option}`)}</span>
                    <span className="set-row-desc">{t(`settings-retention-${option}-desc`)}</span>
                  </span>
                </Radio>
              ))}
            </div>
            <Text slot="description" className="set-row-desc set-note">
              {t('settings-retention-note')}
            </Text>
          </RadioGroup>
          <ErrorText code={retentionError} />
        </Row>
      </Group>

      <Group title={t('settings-group-library')} description={t('settings-library-desc')}>
        <Row wide label={t('settings-library-location')}>
          <div className="set-path">{location.current}</div>
        </Row>
        {location.restartRequired && (
          <p className="set-warn" role="status">
            <InfoIcon aria-hidden />
            {t('settings-library-restart', { path: location.next })}
          </p>
        )}
        {location.overridden && <p className="set-row-desc">{t('settings-library-overridden')}</p>}
        <ErrorText code={locationError} />
        <div className="set-actions">
          <Button className="set-btn" onPress={() => void reveal()}>
            <FolderOpenIcon aria-hidden />
            {t('settings-library-reveal')}
          </Button>
          <Button
            className="set-btn"
            isDisabled={location.overridden}
            onPress={() => void choose()}
          >
            {t('settings-library-change')}
          </Button>
          <Button
            className="set-btn"
            isDisabled={location.overridden || location.next === location.default}
            onPress={() => void move(null)}
          >
            {t('settings-library-default')}
          </Button>
        </div>
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
