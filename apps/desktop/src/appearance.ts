import { invoke, isTauri } from '@tauri-apps/api/core';
import { useSyncExternalStore } from 'react';

/** Themes of Settings › General; `system` follows macOS. */
export const THEMES = ['system', 'light', 'dark'] as const;
export type Theme = (typeof THEMES)[number];

/** Accents of the visual direction (`.acc-<name>` in docs/design/tokens.css). */
export const ACCENTS = [
  'lime',
  'blue',
  'purple',
  'pink',
  'red',
  'orange',
  'yellow',
  'green',
  'graphite',
] as const;
export type Accent = (typeof ACCENTS)[number];

export interface Appearance {
  theme: Theme;
  accent: Accent;
}

const DEFAULT: Appearance = { theme: 'system', accent: 'lime' };

let current: Appearance = DEFAULT;
const listeners = new Set<() => void>();

function isTheme(value: unknown): value is Theme {
  return (THEMES as readonly unknown[]).includes(value);
}

function isAccent(value: unknown): value is Accent {
  return (ACCENTS as readonly unknown[]).includes(value);
}

/** Puts the accent class on <html>, where the theme classes are, replacing the old one. */
function applyAccent(accent: Accent) {
  const root = document.documentElement;
  for (const name of ACCENTS) root.classList.remove(`acc-${name}`);
  root.classList.add(`acc-${accent}`);
}

function publish(next: Appearance) {
  current = next;
  applyAccent(next.accent);
  for (const listener of listeners) listener();
}

/** Shows `next` at once; the caller saves it in the core. */
export function setAppearance(next: Appearance) {
  publish(next);
}

/** Reads the saved appearance before the first render, so the window opens with it. */
export async function loadAppearance(): Promise<void> {
  if (!isTauri()) {
    publish(DEFAULT);
    return;
  }
  try {
    const saved = await invoke<{ theme: unknown; accent: unknown }>('app_settings');
    publish({
      theme: isTheme(saved.theme) ? saved.theme : DEFAULT.theme,
      accent: isAccent(saved.accent) ? saved.accent : DEFAULT.accent,
    });
  } catch (error) {
    console.error('Cannot read the appearance, using the defaults', error);
    publish(DEFAULT);
  }
}

function subscribe(onChange: () => void) {
  listeners.add(onChange);
  return () => listeners.delete(onChange);
}

/** The appearance chosen in Settings. */
export function useAppearance(): Appearance {
  return useSyncExternalStore(subscribe, () => current);
}
