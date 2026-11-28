import { useLayoutEffect } from 'react';
import { useTranslucency } from './shell/useTranslucency';
import { useColorScheme } from './useColorScheme';

/**
 * Puts the theme classes on <html>, so that overlays rendered in portals get the tokens
 * too: the scheme follows the system, the glass goes opaque with Reduce transparency.
 */
export function useThemeClasses() {
  const scheme = useColorScheme();
  const translucent = useTranslucency();
  useLayoutEffect(() => {
    const root = document.documentElement;
    const classes = ['t-app', `t-${scheme}`, translucent ? 't-translucent' : 't-opaque'];
    root.classList.add(...classes);
    return () => root.classList.remove(...classes);
  }, [scheme, translucent]);
}
