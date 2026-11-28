import { isTauri } from '@tauri-apps/api/core';
import { useSyncExternalStore } from 'react';
import { useAppearance } from '../appearance';

const query = '(prefers-reduced-transparency: reduce)';

function subscribe(onChange: () => void) {
  const media = window.matchMedia(query);
  media.addEventListener('change', onChange);
  return () => media.removeEventListener('change', onChange);
}

function reducedTransparency(): boolean {
  return window.matchMedia(query).matches;
}

/** Whether Reduce transparency is on in macOS (System Settings › Accessibility › Display). */
export function useSystemReducedTransparency(): boolean {
  return useSyncExternalStore(subscribe, reducedTransparency);
}

/**
 * Whether the window shows the desktop through the glass frame. Only the macOS app has a
 * translucent window (vibrancy, see tauri.conf.json); a plain browser, other platforms and
 * Reduce transparency (in macOS or in Settings › General) get the opaque fallback. macOS
 * also makes the vibrancy itself opaque when its Reduce transparency is on.
 */
export function useTranslucency(): boolean {
  const reduced = useSystemReducedTransparency();
  const { reduceTransparency } = useAppearance();
  return !reduced && !reduceTransparency && isTauri() && /Mac/.test(navigator.userAgent);
}
