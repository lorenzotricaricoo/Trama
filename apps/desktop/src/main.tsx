import '@fontsource-variable/geist';
import '@fontsource-variable/geist-mono';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import './app.css';
import { negotiateLocale } from './i18n/catalogs';
import { I18nProvider } from './i18n/I18nProvider';
import { requestedLocales } from './i18n/locale';

const root = document.getElementById('root');
if (!root) {
  throw new Error('Missing #root element');
}

const locale = negotiateLocale(await requestedLocales());

createRoot(root).render(
  <StrictMode>
    <I18nProvider locale={locale}>
      <App />
    </I18nProvider>
  </StrictMode>,
);
