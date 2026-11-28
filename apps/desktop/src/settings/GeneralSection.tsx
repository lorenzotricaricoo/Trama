import { useState } from 'react';
import { isSupportedLocale, SUPPORTED_LOCALES } from '../i18n/catalogs';
import { useI18n } from '../i18n/I18nProvider';
import { api, errorCode, type AppSettings } from './api';
import { ErrorText, Group, Segmented } from './ui';

const SYSTEM = 'system';

/** The name of a language written in that language, capitalised as a menu item. */
export function languageName(tag: string, inLocale: string): string {
  const name = new Intl.DisplayNames([inLocale], { type: 'language' }).of(tag) ?? tag;
  return name.charAt(0).toLocaleUpperCase(inLocale) + name.slice(1);
}

interface Props {
  app: AppSettings;
  onApp: (app: AppSettings) => void;
}

/** General: the interface language. */
export function GeneralSection({ app, onApp }: Props) {
  const { t, setLocale } = useI18n();
  const [languageError, setLanguageError] = useState<string | null>(null);

  // Each interface language is written in itself, so it can be found in any language.
  const options = [
    { id: SYSTEM, label: t('settings-language-system') },
    ...SUPPORTED_LOCALES.map((tag) => ({ id: tag, label: languageName(tag, tag) })),
  ];

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

  return (
    <Group
      title={t('settings-interface-language')}
      description={t('settings-interface-language-desc')}
    >
      <Segmented
        label={t('settings-interface-language')}
        labelHidden
        options={options}
        value={app.interfaceLanguage ?? SYSTEM}
        onChange={(value) => void changeLanguage(value)}
      />
      <ErrorText code={languageError} />
    </Group>
  );
}
