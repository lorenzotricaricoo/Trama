import { useEffect, useRef } from 'react';

/** Keyboard shortcuts of the shell. Labels use the macOS symbols, which are not translated. */
export type ShortcutId =
  'commandBar' | 'toggleSidebar' | 'settings' | 'back' | 'record' | 'newNote';

interface Shortcut {
  /** Shown in hint chips. */
  label: string;
  matches: (event: KeyboardEvent) => boolean;
}

/** Whether the app runs on macOS, where the shortcuts use ⌘. */
export function onMac(): boolean {
  return /Mac/.test(navigator.platform) || /Mac OS X/.test(navigator.userAgent);
}

/**
 * ⌘ on macOS, Ctrl elsewhere. On macOS Ctrl is left alone: Ctrl+K, Ctrl+[ and the like are
 * standard text-editing keys there.
 */
const mod = (event: KeyboardEvent) =>
  onMac() ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey;

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
    // The physical key too: on some layouts (Italian) [ needs ⌥.
    matches: (e) => mod(e) && !e.altKey && (e.key === '[' || e.code === 'BracketLeft'),
  },
  record: {
    label: '⌥⌘R',
    // The physical key: with ⌥ held, macOS turns R into another character.
    matches: (e) => mod(e) && e.altKey && e.code === 'KeyR',
  },
  newNote: {
    label: '⌘N',
    // ⌘N has no editing meaning in text fields, so it acts there too, the editor included.
    matches: (e) => mod(e) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'n',
  },
};

/** Text fields where ⌘[ keeps its editing meaning. */
function isTextField(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || target.tagName === 'INPUT' || target.tagName === 'TEXTAREA')
  );
}

/**
 * Runs the handler of a shell shortcut pressed anywhere in the window. Only the shortcuts in
 * `enabled` act (all by default); the others are left to the page.
 */
export function useShortcuts(
  handlers: Record<ShortcutId, () => void>,
  enabled?: readonly ShortcutId[],
) {
  // The latest values, so the listener is added once.
  const latest = useRef({ handlers, enabled });
  useEffect(() => {
    latest.current = { handlers, enabled };
  });

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // Keys pressed while composing (accents, IME) belong to the text being typed.
      if (event.defaultPrevented || event.repeat || event.isComposing) return;
      const id = (Object.keys(SHORTCUTS) as ShortcutId[]).find((key) =>
        SHORTCUTS[key].matches(event),
      );
      if (id === undefined) return;
      const { handlers: current, enabled: allowed } = latest.current;
      if (allowed && !allowed.includes(id)) return;
      if (id === 'back' && isTextField(event.target)) return;
      event.preventDefault();
      current[id]();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);
}
