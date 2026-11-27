import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { App } from './App';
import { I18nProvider } from './i18n/I18nProvider';
import type { Locale } from './i18n/catalogs';

function renderIn(locale: Locale) {
  return render(
    <I18nProvider locale={locale}>
      <App />
    </I18nProvider>,
  );
}

describe('App', () => {
  it('speaks Italian', () => {
    renderIn('it');
    expect(screen.getByRole('heading', { name: 'Benvenuto in Trama' })).toBeInTheDocument();
    expect(screen.getByText(/^Oggi è /)).toBeInTheDocument();
    expect(document.documentElement.lang).toBe('it');
  });

  it('speaks English', () => {
    renderIn('en');
    expect(screen.getByRole('heading', { name: 'Welcome to Trama' })).toBeInTheDocument();
    expect(screen.getByText(/^Today is /)).toBeInTheDocument();
  });

  it('applies the theme tokens to the root', () => {
    const { container } = renderIn('en');
    expect(container.querySelector('main')).toHaveClass('t-app', 't-light');
  });
});
