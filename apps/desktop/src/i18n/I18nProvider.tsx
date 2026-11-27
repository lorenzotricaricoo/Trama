import { createContext, use, useEffect, useMemo, type ReactNode } from 'react';
import { I18nProvider as AriaI18nProvider } from 'react-aria-components';
import { createBundle, createTranslate, type Locale, type Translate } from './catalogs';

interface I18nValue {
  locale: Locale;
  t: Translate;
}

const I18nContext = createContext<I18nValue | null>(null);

export function I18nProvider({ locale, children }: { locale: Locale; children: ReactNode }) {
  const value = useMemo(() => ({ locale, t: createTranslate(createBundle(locale)) }), [locale]);

  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);

  return (
    <I18nContext value={value}>
      <AriaI18nProvider locale={locale}>{children}</AriaI18nProvider>
    </I18nContext>
  );
}

/** The current locale and the message formatter. */
export function useI18n(): I18nValue {
  const value = use(I18nContext);
  if (!value) {
    throw new Error('useI18n must be used inside <I18nProvider>');
  }
  return value;
}
