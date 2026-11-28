import { useMemo, useState } from 'react';
import { Label, Radio, RadioGroup, Switch } from 'react-aria-components';
import { ACCENTS, THEMES, setAppearance, type Accent, type Theme } from '../appearance';
import { isSupportedLocale, SUPPORTED_LOCALES } from '../i18n/catalogs';
import { useI18n } from '../i18n/I18nProvider';
import { useSystemReducedTransparency } from '../shell/useTranslucency';
import { CheckIcon } from '../ui/icons';
import { api, errorCode, type AppSettings, type LibrarySettings } from './api';
import type { UpdateLibrary } from './SettingsScreen';
import { ErrorText, Group, Row, SelectField } from './ui';

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

const SYSTEM = 'system';

/** The name of a language in another language, capitalised as a menu item. */
function languageName(tag: string, inLocale: string): string {
  const name = new Intl.DisplayNames([inLocale], { type: 'language' }).of(tag) ?? tag;
  return name.charAt(0).toLocaleUpperCase(inLocale) + name.slice(1);
}

interface Props {
  app: AppSettings;
  onApp: (app: AppSettings) => void;
  library: LibrarySettings;
  onLibrary: UpdateLibrary;
}

export function GeneralSection({ app, onApp, library, onLibrary }: Props) {
  const { t, locale, setLocale } = useI18n();
  const [languageError, setLanguageError] = useState<string | null>(null);
  const [meetingError, setMeetingError] = useState<string | null>(null);
  const [appearanceError, setAppearanceError] = useState<string | null>(null);
  const [transparencyError, setTransparencyError] = useState<string | null>(null);
  const systemReduced = useSystemReducedTransparency();

  // Each interface language is written in itself, so it can be found in any language.
  const interfaceOptions = [
    { id: SYSTEM, label: t('settings-language-system') },
    ...SUPPORTED_LOCALES.map((tag) => ({ id: tag, label: languageName(tag, tag) })),
  ];
  const meetingOptions = useMemo(
    () =>
      MEETING_LANGUAGES.map((tag) => ({ id: tag, label: languageName(tag, locale) })).sort((a, b) =>
        a.label.localeCompare(b.label, locale),
      ),
    [locale],
  );

  const changeLanguage = async (value: string) => {
    const language = value === SYSTEM ? null : value;
    setLanguageError(null);
    try {
      const negotiated = await api.setInterfaceLanguage(language);
      onApp({ ...app, interfaceLanguage: language });
      if (isSupportedLocale(negotiated)) setLocale(negotiated);
    } catch (error) {
      setLanguageError(errorCode(error));
    }
  };

  const changeAppearance = async (theme: Theme, accent: Accent) => {
    const previous = { theme: app.theme, accent: app.accent };
    const { reduceTransparency } = app;
    setAppearanceError(null);
    setAppearance({ theme, accent, reduceTransparency });
    onApp({ ...app, theme, accent });
    try {
      await api.setAppearance(theme, accent);
    } catch (error) {
      setAppearance({ ...previous, reduceTransparency });
      onApp({ ...app, ...previous });
      setAppearanceError(errorCode(error));
    }
  };

  const changeTransparency = async (reduceTransparency: boolean) => {
    const { theme, accent } = app;
    setTransparencyError(null);
    setAppearance({ theme, accent, reduceTransparency });
    onApp({ ...app, reduceTransparency });
    try {
      await api.setReduceTransparency(reduceTransparency);
    } catch (error) {
      setAppearance({ theme, accent, reduceTransparency: !reduceTransparency });
      onApp({ ...app, reduceTransparency: !reduceTransparency });
      setTransparencyError(errorCode(error));
    }
  };

  return (
    <>
      <Group title={t('settings-group-language')}>
        <Row wide>
          <SelectField
            label={t('settings-interface-language')}
            description={t('settings-interface-language-desc')}
            options={interfaceOptions}
            value={app.interfaceLanguage ?? SYSTEM}
            onChange={(value) => void changeLanguage(value)}
          />
          <ErrorText code={languageError} />
        </Row>
        <Row wide>
          <SelectField
            label={t('settings-meeting-language')}
            description={t('settings-meeting-language-desc')}
            options={meetingOptions}
            value={library.meetingLanguage}
            onChange={(meetingLanguage) => {
              setMeetingError(null);
              void onLibrary({ meetingLanguage }).then(setMeetingError);
            }}
          />
          <ErrorText code={meetingError} />
        </Row>
      </Group>

      <Group title={t('settings-group-appearance')}>
        <Row wide>
          <RadioGroup
            className="set-radios"
            orientation="horizontal"
            value={app.theme}
            onChange={(value) => void changeAppearance(value as Theme, app.accent)}
          >
            <Label className="set-row-label">{t('settings-theme')}</Label>
            <div className="set-segments">
              {THEMES.map((theme) => (
                <Radio key={theme} value={theme} className="set-segment">
                  {t(`settings-theme-${theme}`)}
                </Radio>
              ))}
            </div>
          </RadioGroup>
        </Row>
        <Row wide>
          <RadioGroup
            className="set-radios"
            orientation="horizontal"
            value={app.accent}
            onChange={(value) => void changeAppearance(app.theme, value as Accent)}
          >
            <Label className="set-row-label">{t('settings-accent')}</Label>
            <div className="set-swatches">
              {ACCENTS.map((accent) => (
                <Radio
                  key={accent}
                  value={accent}
                  className={`set-swatch acc-${accent}`}
                  aria-label={t(`settings-accent-${accent}`)}
                >
                  {({ isSelected }) => isSelected && <CheckIcon aria-hidden weight="bold" />}
                </Radio>
              ))}
            </div>
          </RadioGroup>
          <ErrorText code={appearanceError} />
        </Row>
        <Row wide>
          <Switch
            className="set-switch"
            isSelected={systemReduced || app.reduceTransparency}
            onChange={(value) => void changeTransparency(value)}
            isDisabled={systemReduced}
          >
            <span className="set-switch-track" aria-hidden>
              <span className="set-switch-thumb" />
            </span>
            <span className="set-switch-text">
              <span className="set-row-label">{t('settings-reduce-transparency')}</span>
              <span className="set-row-desc">
                {systemReduced
                  ? t('settings-reduce-transparency-system')
                  : t('settings-reduce-transparency-desc')}
              </span>
            </span>
          </Switch>
          <ErrorText code={transparencyError} />
        </Row>
      </Group>
    </>
  );
}
