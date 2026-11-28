import { isTauri } from '@tauri-apps/api/core';
import { useEffect, useState } from 'react';
import { Button } from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import { api } from '../settings/api';
import { useModelStatus } from '../settings/TranscriptionSection';
import { WrenchIcon } from '../ui/icons';
import { onboardingApi, type Permission } from './api';
import './onboarding.css';
import type { Step } from './steps';

/** What the onboarding left for later that recording a meeting needs. */
export type SetupNeed = 'model' | 'provider' | 'microphone';

/** The step of the onboarding that sets each need up. */
export const NEED_STEPS: Record<SetupNeed, Step> = {
  model: 'model',
  provider: 'provider',
  microphone: 'permissions',
};

/** Remembers on this Mac that the user hid the reminder; ⌘K still opens the setup. */
const HIDDEN_KEY = 'trama.setup-reminder-hidden';

/** The needs from what the core reports; `null` until everything has been read. */
export function setupNeeds(
  modelState: string | undefined,
  summaryChosen: boolean | null,
  microphone: Permission | null,
): SetupNeed[] | null {
  if (modelState === undefined || summaryChosen === null || microphone === null) return null;
  const needs: SetupNeed[] = [];
  if (microphone !== 'granted' && microphone !== 'unknown') needs.push('microphone');
  if (modelState !== 'ready' && modelState !== 'downloading' && modelState !== 'verifying') {
    needs.push('model');
  }
  if (!summaryChosen) needs.push('provider');
  return needs;
}

function useSetupNeeds(): SetupNeed[] | null {
  const model = useModelStatus().status;
  const [summaryChosen, setSummaryChosen] = useState<boolean | null>(null);
  const [microphone, setMicrophone] = useState<Permission | null>(null);

  useEffect(() => {
    if (!isTauri()) return;
    let active = true;
    Promise.all([api.taskModels(), onboardingApi.microphone()])
      .then(([tasks, permission]) => {
        if (!active) return;
        setSummaryChosen(tasks.some((task) => task.task === 'summary'));
        setMicrophone(permission);
      })
      .catch((error: unknown) => console.error('Cannot read what the setup left', error));
    return () => {
      active = false;
    };
  }, []);

  return setupNeeds(model?.state, summaryChosen, microphone);
}

/**
 * Home: what the onboarding left for later, with a way back to the step that sets it up.
 * Gone once everything is set up, or when the user hides it.
 */
export function SetupTile({ onOpen }: { onOpen: (step: Step) => void }) {
  const { t } = useI18n();
  const needs = useSetupNeeds();
  const [hidden, setHidden] = useState(() => localStorage.getItem(HIDDEN_KEY) === '1');
  const first = needs?.[0];
  if (hidden || first === undefined) return null;

  return (
    <section className="setup-tile" aria-label={t('onboarding-home-title')}>
      <span className="setup-tile-ic" aria-hidden>
        <WrenchIcon />
      </span>
      <div className="setup-tile-text">
        <h2 className="setup-tile-title">{t('onboarding-home-title')}</h2>
        <ul className="setup-tile-list">
          {needs?.map((need) => (
            <li key={need}>{t(`onboarding-home-${need}`)}</li>
          ))}
        </ul>
      </div>
      <div className="setup-tile-actions">
        <Button
          className="set-btn"
          data-variant="quiet"
          onPress={() => {
            localStorage.setItem(HIDDEN_KEY, '1');
            setHidden(true);
          }}
        >
          {t('onboarding-home-hide')}
        </Button>
        <Button
          className="set-btn"
          data-variant="primary"
          onPress={() => onOpen(NEED_STEPS[first])}
        >
          {t('onboarding-home-continue')}
        </Button>
      </div>
    </section>
  );
}
