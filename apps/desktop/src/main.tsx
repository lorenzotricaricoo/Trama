import '@fontsource-variable/geist';
import '@fontsource-variable/geist-mono';
import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import './app.css';
import { negotiateLocale } from './i18n/catalogs';
import { I18nProvider } from './i18n/I18nProvider';
import { requestedLocales } from './i18n/locale';
import { RecordingPanel } from './recording/RecordingPanel';

const root = document.getElementById('root');
if (!root) {
  throw new Error('Missing #root element');
}

const locale = negotiateLocale(await requestedLocales());
// The recording panel is a window of its own on the same page (see `recording.rs`).
const panel = isTauri() && getCurrentWindow().label === 'recording';

createRoot(root).render(
  <StrictMode>
    <I18nProvider locale={locale}>{panel ? <RecordingPanel /> : <App />}</I18nProvider>
  </StrictMode>,
);
