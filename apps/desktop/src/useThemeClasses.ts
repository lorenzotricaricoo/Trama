import { useLayoutEffect } from 'react';
import { useAppearance } from './appearance';
import { useTranslucency } from './shell/useTranslucency';
import { useColorScheme } from './useColorScheme';

/**
 * Puts the theme classes on <html>, so that overlays rendered in portals get the tokens
 * too: the scheme and the glass follow the appearance settings, and Reduce motion calms
 * the animations.
 */
export function useThemeClasses() {
  const scheme = useColorScheme();
  const translucent = useTranslucency();
  const { reduceMotion } = useAppearance();
  useLayoutEffect(() => {
    const root = document.documentElement;
    const classes = ['t-app', `t-${scheme}`, translucent ? 't-translucent' : 't-opaque'];
    if (reduceMotion) classes.push('t-calm');
    root.classList.add(...classes);
    return () => root.classList.remove(...classes);
  }, [scheme, translucent, reduceMotion]);
}
