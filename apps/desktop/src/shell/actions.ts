import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useEffect, useState } from 'react';
import { SHORTCUTS, useShortcuts, type ShortcutId } from './shortcuts';

/**
 * Actions of the shell, run by a shortcut or by the native menu. The ids are the menu item
 * ids in `src-tauri/src/menu.rs`.
 */
export type ActionId = ShortcutId | 'home' | 'sources';

export const ACTION_IDS: readonly ActionId[] = [
  'newNote',
  'record',
  'toggleSidebar',
  'home',
  'sources',
  'back',
  'commandBar',
  'settings',
];

/** Actions that still run while the command bar is open; they close it. */
export const COMMAND_BAR_ACTIONS: readonly ActionId[] = [
  'commandBar',
  'settings',
  'newNote',
  'home',
  'sources',
];

/** Sent by the shell when a shell command is chosen in the native menu. */
export const MENU_COMMAND_EVENT = 'menu-command';

/**
 * How long after a shortcut was pressed in the webview the menu item with the same shortcut
 * is taken as that same key press. macOS gives the page the first go at a key equivalent
 * and passes it to the menu only when the page leaves it alone; the menu event arrives a
 * moment later.
 */
export const SAME_KEY_PRESS_MS = 500;

function isActionId(id: unknown): id is ActionId {
  return typeof id === 'string' && (ACTION_IDS as readonly string[]).includes(id);
}

/** The single place where a shell action runs, whichever way it was asked for. */
export interface ActionDispatcher {
  /** Sets what running an action does: the latest handlers of the shell. */
  setRunner: (run: (id: ActionId) => void) => void;
  /** Runs an action asked for by a shortcut the webview handled. */
  run: (id: ActionId) => void;
  /** Notes a shell shortcut pressed in the webview, run or not. */
  keyPressed: (id: ShortcutId) => void;
  /** Runs an action chosen in the native menu, unless it is the key press just handled. */
  fromMenu: (id: unknown) => void;
}

export function createActionDispatcher(
  now: () => number = () => performance.now(),
): ActionDispatcher {
  let runner: (id: ActionId) => void = () => {};
  let lastKey: { id: ShortcutId; at: number } | undefined;
  return {
    setRunner: (run) => {
      runner = run;
    },
    run: (id) => runner(id),
    keyPressed: (id) => {
      lastKey = { id, at: now() };
    },
    fromMenu: (id) => {
      if (!isActionId(id)) return;
      // The webview already had this key press: it ran the action or chose not to (held
      // down, in a text field, behind the command bar). Running it again would undo it.
      if (lastKey?.id === id && now() - lastKey.at < SAME_KEY_PRESS_MS) {
        lastKey = undefined;
        return;
      }
      runner(id);
    },
  };
}

/** The actions that can run now, so the menu can disable the others. */
export function enabledActions(options: {
  commandBarOpen: boolean;
  canGoBack: boolean;
}): ActionId[] {
  return ACTION_IDS.filter(
    (id) =>
      (!options.commandBarOpen || COMMAND_BAR_ACTIONS.includes(id)) &&
      (id !== 'back' || options.canGoBack),
  );
}

const SHORTCUT_IDS = Object.keys(SHORTCUTS) as ShortcutId[];

/**
 * Runs the shell actions from their shortcuts and from the native menu, through one
 * dispatcher, so a key press runs its action once. Only the actions in `enabled` run (all
 * by default).
 */
export function useShellActions(
  handlers: Record<ActionId, () => void>,
  enabled?: readonly ActionId[],
) {
  const [dispatcher] = useState(() => createActionDispatcher());
  // The latest handlers, so the dispatcher and the listeners are created once.
  useEffect(() => {
    dispatcher.setRunner((id) => {
      if (enabled && !enabled.includes(id)) return;
      handlers[id]();
    });
  });
  const [keyHandlers] = useState(
    () =>
      Object.fromEntries(SHORTCUT_IDS.map((id) => [id, () => dispatcher.run(id)])) as Record<
        ShortcutId,
        () => void
      >,
  );

  useShortcuts(
    keyHandlers,
    enabled?.filter((id): id is ShortcutId => id !== 'home' && id !== 'sources'),
    dispatcher.keyPressed,
  );

  useEffect(() => {
    if (!isTauri()) return;
    const unlisten = listen<unknown>(MENU_COMMAND_EVENT, (event) => {
      dispatcher.fromMenu(event.payload);
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [dispatcher]);
}

/** What the native menu shows for the shell commands (see `set_menu_state`). */
export interface MenuState {
  enabled: ActionId[];
  recording: boolean;
  sidebarCollapsed: boolean;
}

/** Keeps the native menu in step: disabled items, Record or Stop, Show or Hide Sidebar. */
export function useMenuState(state: MenuState) {
  const key = JSON.stringify(state);
  useEffect(() => {
    if (!isTauri()) return;
    invoke('set_menu_state', { state: JSON.parse(key) as MenuState }).catch((error: unknown) => {
      console.error('Cannot update the menu', error);
    });
  }, [key]);
}
