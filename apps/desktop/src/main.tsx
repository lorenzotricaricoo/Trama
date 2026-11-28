import '@fontsource-variable/geist';
import '@fontsource-variable/geist-mono';
import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { loadAppearance } from './appearance';
import './app.css';
import { negotiateLocale } from './i18n/catalogs';
import { I18nProvider } from './i18n/I18nProvider';
import { requestedLocales } from './i18n/locale';
import { onboardingApi } from './onboarding/api';
import { resumeStep } from './onboarding/steps';
import { RecordingPanel } from './recording/RecordingPanel';

const root = document.getElementById('root');
if (!root) {
  throw new Error('Missing #root element');
}

// The recording panel is a window of its own on the same page (see `recording.rs`).
const panel = isTauri() && getCurrentWindow().label === 'recording';

/** The onboarding step to start at; `null` when it is done. Read before the first render. */
async function loadOnboarding() {
  if (!isTauri() || panel) return null;
  try {
    const { pending } = await onboardingApi.state();
    return pending === null ? null : resumeStep(pending);
  } catch (error) {
    console.error('Cannot read the onboarding state', error);
    return null;
  }
}

// The language, the appearance and the onboarding are read before the first render, so
// nothing flashes.
const [requested, , onboarding] = await Promise.all([
  requestedLocales(),
  loadAppearance(),
  loadOnboarding(),
]);
const locale = negotiateLocale(requested);

createRoot(root).render(
  <StrictMode>
    <I18nProvider locale={locale}>
      {panel ? <RecordingPanel /> : <App onboarding={onboarding} />}
    </I18nProvider>
  </StrictMode>,
);
