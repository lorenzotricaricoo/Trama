import { act, fireEvent, renderHook } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest';
import {
  ACTION_IDS,
  MENU_COMMAND_EVENT,
  SAME_KEY_PRESS_MS,
  createActionDispatcher,
  enabledActions,
  useMenuState,
  useShellActions,
  type ActionId,
} from './actions';

const tauri = vi.hoisted(() => ({
  handlers: new Map<string, (event: { payload: unknown }) => void>(),
  invoke: vi.fn<(cmd: string, args?: unknown) => Promise<null>>(() => Promise.resolve(null)),
}));

vi.mock('@tauri-apps/api/core', () => ({
  isTauri: () => true,
  invoke: tauri.invoke,
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((name: string, handler: (event: { payload: unknown }) => void) => {
    tauri.handlers.set(name, handler);
    return Promise.resolve(() => tauri.handlers.delete(name));
  }),
}));

beforeEach(() => {
  tauri.handlers.clear();
  tauri.invoke.mockClear();
});

describe('createActionDispatcher', () => {
  function setup() {
    let time = 1_000;
    const run = vi.fn<(id: ActionId) => void>();
    const dispatcher = createActionDispatcher(() => time);
    dispatcher.setRunner(run);
    return { run, dispatcher, advance: (ms: number) => (time += ms) };
  }

  it('runs a menu choice', () => {
    const { run, dispatcher } = setup();
    dispatcher.fromMenu('home');
    expect(run).toHaveBeenCalledExactlyOnceWith('home');
  });

  it('does not run again the key press the webview handled', () => {
    const { run, dispatcher, advance } = setup();
    dispatcher.keyPressed('toggleSidebar');
    dispatcher.run('toggleSidebar');
    advance(20);
    // macOS passes the same key equivalent to the menu.
    dispatcher.fromMenu('toggleSidebar');
    expect(run).toHaveBeenCalledExactlyOnceWith('toggleSidebar');
  });

  it('leaves a key press the webview declined to the webview', () => {
    const { run, dispatcher, advance } = setup();
    // ⌘[ in a text field: neither the page nor the menu goes back.
    dispatcher.keyPressed('back');
    advance(20);
    dispatcher.fromMenu('back');
    expect(run).not.toHaveBeenCalled();
  });

  it('runs a later menu choice, or another item, normally', () => {
    const { run, dispatcher, advance } = setup();
    dispatcher.keyPressed('record');
    dispatcher.run('record');
    dispatcher.fromMenu('settings');
    advance(SAME_KEY_PRESS_MS);
    dispatcher.fromMenu('record');
    expect(run.mock.calls).toEqual([['record'], ['settings'], ['record']]);
  });

  it('takes a key press as one menu choice only', () => {
    const { run, dispatcher } = setup();
    dispatcher.keyPressed('commandBar');
    dispatcher.fromMenu('commandBar');
    dispatcher.fromMenu('commandBar');
    expect(run).toHaveBeenCalledExactlyOnceWith('commandBar');
  });

  it('ignores unknown menu ids', () => {
    const { run, dispatcher } = setup();
    dispatcher.fromMenu('quit');
    dispatcher.fromMenu(42);
    expect(run).not.toHaveBeenCalled();
  });
});

describe('enabledActions', () => {
  it('disables Back without history', () => {
    expect(enabledActions({ commandBarOpen: false, canGoBack: false })).toEqual(
      ACTION_IDS.filter((id) => id !== 'back'),
    );
    expect(enabledActions({ commandBarOpen: false, canGoBack: true })).toEqual(ACTION_IDS);
  });

  it('keeps only the command bar actions while it is open', () => {
    expect(enabledActions({ commandBarOpen: true, canGoBack: true }).sort()).toEqual(
      ['commandBar', 'home', 'importNotes', 'newNote', 'settings', 'sources'].sort(),
    );
  });
});

describe('useShellActions', () => {
  function handlers() {
    return Object.fromEntries(ACTION_IDS.map((id) => [id, vi.fn<() => void>()])) as Record<
      ActionId,
      Mock<() => void>
    >;
  }
  const menu = (id: string) => act(() => tauri.handlers.get(MENU_COMMAND_EVENT)?.({ payload: id }));
  const toggleSidebarKey = () =>
    fireEvent.keyDown(window, { key: '\\', code: 'Backslash', metaKey: true });

  it('runs a shortcut once when the menu item with that shortcut follows', async () => {
    const run = handlers();
    renderHook(() => useShellActions(run));
    await act(() => Promise.resolve());
    toggleSidebarKey();
    menu('toggleSidebar');
    expect(run.toggleSidebar).toHaveBeenCalledOnce();
  });

  it('runs menu choices, also those without a shortcut', async () => {
    const run = handlers();
    renderHook(() => useShellActions(run));
    await act(() => Promise.resolve());
    menu('sources');
    menu('toggleSidebar');
    expect(run.sources).toHaveBeenCalledOnce();
    expect(run.toggleSidebar).toHaveBeenCalledOnce();
  });

  it('runs neither a disabled shortcut nor its menu item', async () => {
    const run = handlers();
    renderHook(() => useShellActions(run, ['commandBar', 'home']));
    await act(() => Promise.resolve());
    toggleSidebarKey();
    menu('toggleSidebar');
    menu('home');
    expect(run.toggleSidebar).not.toHaveBeenCalled();
    expect(run.home).toHaveBeenCalledOnce();
  });

  it('does not let a held shortcut repeat through the menu', async () => {
    const run = handlers();
    renderHook(() => useShellActions(run));
    await act(() => Promise.resolve());
    toggleSidebarKey();
    menu('toggleSidebar');
    fireEvent.keyDown(window, { key: '\\', code: 'Backslash', metaKey: true, repeat: true });
    menu('toggleSidebar');
    expect(run.toggleSidebar).toHaveBeenCalledOnce();
  });
});

describe('useMenuState', () => {
  it('reports the menu state when it changes', () => {
    const state = { enabled: ['home' as const], recording: false, sidebarCollapsed: false };
    const { rerender } = renderHook((props) => useMenuState(props), { initialProps: state });
    rerender({ ...state });
    expect(tauri.invoke).toHaveBeenCalledExactlyOnceWith('set_menu_state', { state });
    rerender({ ...state, recording: true });
    expect(tauri.invoke).toHaveBeenLastCalledWith('set_menu_state', {
      state: { ...state, recording: true },
    });
  });
});
