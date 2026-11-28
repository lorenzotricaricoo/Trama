import { useEffect, useRef } from 'react';

/** Keyboard shortcuts of the shell. Labels use the macOS symbols, which are not translated. */
export type ShortcutId = 'commandBar' | 'toggleSidebar' | 'settings' | 'back' | 'record';

interface Shortcut {
  /** Shown in hint chips. */
  label: string;
  matches: (event: KeyboardEvent) => boolean;
}

/** ⌘ on macOS; Ctrl is accepted too, for other platforms. */
const mod = (event: KeyboardEvent) => event.metaKey || event.ctrlKey;

export const SHORTCUTS: Record<ShortcutId, Shortcut> = {
  commandBar: {
    label: '⌘K',
    matches: (e) => mod(e) && !e.altKey && e.key.toLowerCase() === 'k',
  },
  toggleSidebar: {
    label: '⌘\\',
    matches: (e) => mod(e) && !e.altKey && (e.code === 'Backslash' || e.key === '\\'),
  },
  settings: {
    label: '⌘,',
    matches: (e) => mod(e) && !e.altKey && e.key === ',',
  },
  back: {
    label: '⌘[',
    matches: (e) => mod(e) && !e.altKey && e.key === '[',
  },
  record: {
    label: '⌥⌘R',
    // The physical key: with ⌥ held, macOS turns R into another character.
    matches: (e) => mod(e) && e.altKey && e.code === 'KeyR',
  },
};

/** Text fields where ⌘[ keeps its editing meaning. */
function isTextField(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || target.tagName === 'INPUT' || target.tagName === 'TEXTAREA')
  );
}

/** Runs the handler of a shell shortcut pressed anywhere in the window. */
export function useShortcuts(handlers: Record<ShortcutId, () => void>) {
  // The latest handlers, so the listener is added once.
  const latest = useRef(handlers);
  useEffect(() => {
    latest.current = handlers;
  });

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.repeat) return;
      const id = (Object.keys(SHORTCUTS) as ShortcutId[]).find((key) =>
        SHORTCUTS[key].matches(event),
      );
      if (id === undefined) return;
      if (id === 'back' && isTextField(event.target)) return;
      event.preventDefault();
      latest.current[id]();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);
}
