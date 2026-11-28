import { createContext, use, useEffect, useMemo, useState, type ReactNode } from 'react';
import { I18nProvider as AriaI18nProvider } from 'react-aria-components';
import { createBundle, createTranslate, type Locale, type Translate } from './catalogs';

interface I18nValue {
  locale: Locale;
  t: Translate;
  /** Whether the catalogs have a message, to pick between codes without logging gaps. */
  has: (id: string) => boolean;
  /** Switches the interface to another locale (Settings › General). */
  setLocale: (locale: Locale) => void;
}

const I18nContext = createContext<I18nValue | null>(null);

export function I18nProvider({
  locale: initial,
  children,
}: {
  locale: Locale;
  children: ReactNode;
}) {
  const [locale, setLocale] = useState(initial);
  // A new locale from the parent wins over the one chosen here.
  const [prevInitial, setPrevInitial] = useState(initial);
  if (initial !== prevInitial) {
    setPrevInitial(initial);
    setLocale(initial);
  }
  const value = useMemo(() => {
    const bundle = createBundle(locale);
    return {
      locale,
      t: createTranslate(bundle),
      has: (id: string) => bundle.hasMessage(id),
      setLocale,
    };
  }, [locale]);

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
