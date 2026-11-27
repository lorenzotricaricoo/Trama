import { invoke, isTauri } from '@tauri-apps/api/core';

/**
 * Locales to negotiate. Inside the app the core decides (system language or the
 * TRAMA_LOCALE override), so the native menus and the interface agree; in a plain
 * browser the browser's languages are used.
 */
export async function requestedLocales(): Promise<string[]> {
  if (isTauri()) {
    try {
      return [await invoke<string>('app_locale')];
    } catch (error) {
      console.error('Cannot read the app locale, using the browser languages', error);
    }
  }
  return [...navigator.languages];
}
