import { isTauri } from '@tauri-apps/api/core';
import { useSyncExternalStore } from 'react';

const query = '(prefers-reduced-transparency: reduce)';

function subscribe(onChange: () => void) {
  const media = window.matchMedia(query);
  media.addEventListener('change', onChange);
  return () => media.removeEventListener('change', onChange);
}

function reducedTransparency(): boolean {
  return window.matchMedia(query).matches;
}

/**
 * Whether the window shows the desktop through the glass frame. Only the macOS app has a
 * translucent window (vibrancy, see tauri.conf.json); a plain browser, other platforms and
 * Reduce transparency get the opaque fallback. macOS also makes the vibrancy itself opaque
 * when Reduce transparency is on.
 */
export function useTranslucency(): boolean {
  const reduced = useSyncExternalStore(subscribe, reducedTransparency);
  return !reduced && isTauri() && /Mac/.test(navigator.userAgent);
}
