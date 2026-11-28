import { useSyncExternalStore } from 'react';
import { useAppearance } from './appearance';

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

/** The theme chosen in Settings, or the system appearance when it follows the system. */
export function useColorScheme(): ColorScheme {
  const system = useSyncExternalStore(subscribe, snapshot);
  const { theme } = useAppearance();
  return theme === 'system' ? system : theme;
}
