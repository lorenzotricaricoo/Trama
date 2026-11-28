import { useState, useSyncExternalStore } from 'react';
import { Radio, RadioGroup } from 'react-aria-components';
import {
  ACCENTS,
  currentAppearance,
  setAppearance,
  THEMES,
  type Accent,
  type Appearance,
  type Theme,
} from '../appearance';
import { useI18n } from '../i18n/I18nProvider';
import { useSystemReducedTransparency } from '../shell/useTranslucency';
import { CheckIcon } from '../ui/icons';
import { api, errorCode, type AppSettings } from './api';
import { ErrorText, Group, ToggleRow } from './ui';

const MOTION_QUERY = '(prefers-reduced-motion: reduce)';

function subscribeMotion(onChange: () => void) {
  const media = window.matchMedia(MOTION_QUERY);
  media.addEventListener('change', onChange);
  return () => media.removeEventListener('change', onChange);
}

/** Whether Reduce motion is on in macOS (System Settings › Accessibility › Display). */
function useSystemReducedMotion(): boolean {
  return useSyncExternalStore(subscribeMotion, () => window.matchMedia(MOTION_QUERY).matches);
}

interface Props {
  app: AppSettings;
  onApp: (app: AppSettings) => void;
}

type Saved = Pick<AppSettings, 'theme' | 'accent' | 'reduceTransparency' | 'reduceMotion'>;

/** Appearance: theme, accent colour, and the two accessibility switches. */
export function AppearanceSection({ app, onApp }: Props) {
  const { t } = useI18n();
  const [themeError, setThemeError] = useState<string | null>(null);
  const [accessError, setAccessError] = useState<string | null>(null);
  const systemTransparency = useSystemReducedTransparency();
  const systemMotion = useSystemReducedMotion();

  /** Shows a change at once, saves it, and puts back what was there if saving fails. */
  const change = async (
    next: Partial<Saved>,
    save: () => Promise<void>,
    setError: (code: string | null) => void,
  ) => {
    const before: Appearance = currentAppearance();
    const previous = { ...app };
    setError(null);
    setAppearance(next);
    onApp({ ...app, ...next });
    try {
      await save();
    } catch (error) {
      setAppearance(before);
      onApp(previous);
      setError(errorCode(error));
    }
  };

  const chooseTheme = (theme: Theme) =>
    void change({ theme }, () => api.setAppearance(theme, app.accent), setThemeError);

  return (
    <>
      <Group title={t('settings-theme')}>
        <RadioGroup
          className="set-themes"
          orientation="horizontal"
          aria-label={t('settings-theme')}
          value={app.theme}
          onChange={(value) => chooseTheme(value as Theme)}
        >
          {THEMES.map((theme) => (
            <Radio key={theme} value={theme} className="set-theme">
              <span className="set-theme-preview" data-theme={theme} aria-hidden>
                <span className="set-theme-side" />
                <span className="set-theme-main" />
              </span>
              <span className="set-theme-name">{t(`settings-theme-${theme}`)}</span>
            </Radio>
          ))}
        </RadioGroup>
        <ErrorText code={themeError} />
      </Group>

      <Group title={t('settings-accent')} description={t('settings-accent-desc')}>
        <AccentPicker app={app} onApp={onApp} />
      </Group>

      <Group title={t('settings-group-accessibility')}>
        <ToggleRow
          label={t('settings-reduce-motion')}
          description={
            systemMotion ? t('settings-reduce-motion-system') : t('settings-reduce-motion-desc')
          }
          isSelected={systemMotion || app.reduceMotion}
          isDisabled={systemMotion}
          onChange={(reduceMotion) =>
            void change({ reduceMotion }, () => api.setReduceMotion(reduceMotion), setAccessError)
          }
        />
        <ToggleRow
          label={t('settings-reduce-transparency')}
          description={
            systemTransparency
              ? t('settings-reduce-transparency-system')
              : t('settings-reduce-transparency-desc')
          }
          isSelected={systemTransparency || app.reduceTransparency}
          isDisabled={systemTransparency}
          onChange={(reduceTransparency) =>
            void change(
              { reduceTransparency },
              () => api.setReduceTransparency(reduceTransparency),
              setAccessError,
            )
          }
        />
        <ErrorText code={accessError} />
      </Group>
    </>
  );
}

/**
 * The accent colours with a preview of the accent on a button, a badge and a bar. The change
 * shows at once and is put back if it can't be saved.
 */
export function AccentPicker({ app, onApp, preview = true }: Props & { preview?: boolean }) {
  const { t } = useI18n();
  const [error, setError] = useState<string | null>(null);

  const chooseAccent = async (accent: Accent) => {
    const before: Appearance = currentAppearance();
    const previous = { ...app };
    setError(null);
    setAppearance({ accent });
    onApp({ ...app, accent });
    try {
      await api.setAppearance(app.theme, accent);
    } catch (err) {
      setAppearance(before);
      onApp(previous);
      setError(errorCode(err));
    }
  };

  return (
    <>
      <RadioGroup
        className="set-accents"
        orientation="horizontal"
        aria-label={t('settings-accent')}
        value={app.accent}
        onChange={(value) => void chooseAccent(value as Accent)}
      >
        {ACCENTS.map((accent) => (
          <Radio key={accent} value={accent} className={`set-accent acc-${accent}`}>
            {({ isSelected }) => (
              <>
                <span className="set-swatch" aria-hidden>
                  {isSelected && <CheckIcon weight="bold" />}
                </span>
                <span className="set-accent-name">{t(`settings-accent-${accent}`)}</span>
              </>
            )}
          </Radio>
        ))}
      </RadioGroup>
      {preview && (
        <div className="set-preview" aria-hidden>
          <span className="set-preview-btn">{t('record-meeting')}</span>
          <span className="set-preview-badge">5</span>
          <span className="set-preview-bar">
            <span />
          </span>
          <span className="set-preview-label">{t('settings-accent-preview')}</span>
        </div>
      )}
      <ErrorText code={error} />
    </>
  );
}
