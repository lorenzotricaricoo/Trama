import { describe, expect, it, vi } from 'vitest';
import { createBundle, createTranslate, negotiateLocale } from './catalogs';

describe('negotiateLocale', () => {
  it('matches regional variants to supported locales', () => {
    expect(negotiateLocale(['it-IT'])).toBe('it');
    expect(negotiateLocale(['de-DE', 'en-GB'])).toBe('en');
  });

  it('falls back to English', () => {
    expect(negotiateLocale(['fr-FR'])).toBe('en');
    expect(negotiateLocale([])).toBe('en');
  });
});

describe('createTranslate', () => {
  it('formats messages with arguments and terms', () => {
    const t = createTranslate(createBundle('it'));
    expect(t('welcome-title')).toBe('Benvenuto in Trama');
    expect(t('job-more', { count: 2 })).toBe('+2');
  });

  it('returns the id of a missing message and reports it', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    const t = createTranslate(createBundle('en'));
    expect(t('no-such-message')).toBe('no-such-message');
    expect(error).toHaveBeenCalledOnce();
    error.mockRestore();
  });
});
