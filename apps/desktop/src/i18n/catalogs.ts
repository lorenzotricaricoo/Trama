import { FluentBundle, FluentResource, type FluentVariable } from '@fluent/bundle';
import { negotiateLanguages } from '@fluent/langneg';
// Catalogs are shared with the Rust side; see locales/ at the repository root.
import enApp from '../../../../locales/en/app.ftl?raw';
import enProviders from '../../../../locales/en/providers.ftl?raw';
import enSettings from '../../../../locales/en/settings.ftl?raw';
import itApp from '../../../../locales/it/app.ftl?raw';
import itProviders from '../../../../locales/it/providers.ftl?raw';
import itSettings from '../../../../locales/it/settings.ftl?raw';

export const SUPPORTED_LOCALES = ['en', 'it'] as const;
export type Locale = (typeof SUPPORTED_LOCALES)[number];
export const DEFAULT_LOCALE: Locale = 'en';

/** The catalogs of each locale; the provider error codes are shared with the core. */
const SOURCES: Record<Locale, readonly string[]> = {
  en: [enApp, enSettings, enProviders],
  it: [itApp, itSettings, itProviders],
};

export type MessageArgs = Record<string, FluentVariable>;
export type Translate = (id: string, args?: MessageArgs) => string;

function isLocale(tag: string): tag is Locale {
  return (SUPPORTED_LOCALES as readonly string[]).includes(tag);
}

/** Picks the supported locale that best matches the requested BCP 47 tags. */
export function negotiateLocale(requested: readonly string[]): Locale {
  const [best] = negotiateLanguages(requested, SUPPORTED_LOCALES, {
    defaultLocale: DEFAULT_LOCALE,
    strategy: 'lookup',
  });
  return best !== undefined && isLocale(best) ? best : DEFAULT_LOCALE;
}

/** Whether `tag` is a supported locale. */
export function isSupportedLocale(tag: string): tag is Locale {
  return isLocale(tag);
}

/** Builds the Fluent bundle of a locale. Syntax errors and duplicate ids in a catalog throw. */
export function createBundle(locale: Locale): FluentBundle {
  // Supported locales are all left-to-right, so no Unicode isolation marks.
  const bundle = new FluentBundle(locale, { useIsolating: false });
  for (const source of SOURCES[locale]) {
    const errors = bundle.addResource(new FluentResource(source));
    if (errors.length > 0) {
      throw new Error(`Invalid catalog for ${locale}: ${errors.map(String).join('; ')}`);
    }
  }
  return bundle;
}

/**
 * Returns a function that formats messages. A missing or broken message returns its
 * id, so the gap is visible on screen and reported in the console.
 */
export function createTranslate(bundle: FluentBundle): Translate {
  return (id, args) => {
    const pattern = bundle.getMessage(id)?.value;
    if (!pattern) {
      console.error(`Missing message "${id}" in ${bundle.locales.join(', ')}`);
      return id;
    }
    const errors: Error[] = [];
    const text = bundle.formatPattern(pattern, args ?? null, errors);
    if (errors.length > 0) {
      console.error(`Cannot format message "${id}":`, errors);
      return id;
    }
    return text;
  };
}
