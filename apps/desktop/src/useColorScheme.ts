import { useSyncExternalStore } from 'react';

export type ColorScheme = 'light' | 'dark';

const query = '(prefers-color-scheme: dark)';

function subscribe(onChange: () => void) {
  const media = window.matchMedia(query);
  media.addEventListener('change', onChange);
  return () => media.removeEventListener('change', onChange);
}

function snapshot(): ColorScheme {
  return window.matchMedia(query).matches ? 'dark' : 'light';
}

/** Follows the system appearance. */
export function useColorScheme(): ColorScheme {
  return useSyncExternalStore(subscribe, snapshot);
}
