import { useCallback, useState } from 'react';
import { GearIcon, HouseIcon, StackIcon, type Icon } from '../ui/icons';

/**
 * Screens of the app (see docs/design/information-architecture.md). Only phase 1 screens
 * exist: Projects, People and Doubts join in phase 2, Chat in phase 3, each added here when
 * its phase ships. A note is opened from Sources or with ⌘N, a meeting from Sources or when
 * its recording stops; neither from the sidebar.
 */
export type ScreenId = 'home' | 'sources' | 'settings' | 'note' | 'meeting';

/** Screens that show one source, below Sources in the sidebar. */
const SOURCE_SCREENS: readonly ScreenId[] = ['note', 'meeting'];

export interface NavItem {
  id: Exclude<ScreenId, 'note' | 'meeting'>;
  /** Message id of the label in the catalogs. */
  label: string;
  icon: Icon;
  /** Where the item sits: the main list or the bottom of the sidebar. */
  place: 'main' | 'bottom';
}

/** Sidebar items, in display order. */
export const NAV_ITEMS: readonly NavItem[] = [
  { id: 'home', label: 'nav-home', icon: HouseIcon, place: 'main' },
  { id: 'sources', label: 'nav-sources', icon: StackIcon, place: 'main' },
  { id: 'settings', label: 'nav-settings', icon: GearIcon, place: 'bottom' },
];

/** The sidebar item a screen belongs to: a note or a meeting is a source. */
export function navItemOf(screen: ScreenId): NavItem['id'] {
  return screen === 'note' || screen === 'meeting' ? 'sources' : screen;
}

/** Position of a screen in the sidebar, used for the direction of the screen transition. */
function order(screen: ScreenId): number {
  const index = NAV_ITEMS.findIndex((item) => item.id === navItemOf(screen));
  // A source sits just below Sources.
  return SOURCE_SCREENS.includes(screen) ? index + 0.5 : index;
}

/** Direction of a transition: down when moving further down the sidebar, up otherwise. */
export type Direction = 'down' | 'up';

export interface Navigation {
  screen: ScreenId;
  /** The note shown by the note screen; undefined for a new note not saved yet. */
  noteId: string | undefined;
  /** The meeting shown by the meeting screen. */
  meetingId: string | undefined;
  /** Changes at every navigation, even between two notes: a key for the screen transition. */
  entry: number;
  direction: Direction;
  canGoBack: boolean;
  /**
   * Goes to a screen; for the note screen, to a note or (without an id) to a new one; for
   * the meeting screen, to a meeting.
   */
  navigate: (screen: ScreenId, id?: string) => void;
  back: () => void;
  /**
   * Records the id of a new note once it is saved, in the navigation entry that created it
   * (on screen or in the history), without a transition.
   */
  setNoteId: (entry: number, noteId: string) => void;
}

const MAX_HISTORY = 40;

interface Route {
  screen: ScreenId;
  /** The source shown by the note or meeting screen. */
  id?: string;
  entry: number;
}

interface NavState extends Route {
  history: Route[];
  direction: Direction;
}

let lastEntry = 0;
const nextEntry = () => ++lastEntry;

function towards(from: ScreenId, to: ScreenId): Direction {
  return order(to) >= order(from) ? 'down' : 'up';
}

/** The current screen and the history behind the Back button. */
export function useNavigation(initial: ScreenId = 'home'): Navigation {
  const [state, setState] = useState<NavState>(() => ({
    screen: initial,
    entry: nextEntry(),
    history: [],
    direction: 'down',
  }));

  const navigate = useCallback((screen: ScreenId, id?: string) => {
    setState((current) => {
      if (current.screen === screen && current.id === id) return current;
      const route: Route = {
        screen: current.screen,
        id: current.id,
        entry: current.entry,
      };
      return {
        screen,
        id,
        entry: nextEntry(),
        history: [...current.history, route].slice(-MAX_HISTORY),
        direction: towards(current.screen, screen),
      };
    });
  }, []);

  const back = useCallback(() => {
    setState((current) => {
      const previous = current.history.at(-1);
      return previous === undefined
        ? current
        : {
            ...previous,
            history: current.history.slice(0, -1),
            direction: towards(current.screen, previous.screen),
          };
    });
  }, []);

  const setNoteId = useCallback((entry: number, noteId: string) => {
    const update = <T extends Route>(route: T): T =>
      route.entry === entry && route.screen === 'note' ? { ...route, id: noteId } : route;
    setState((current) => ({ ...update(current), history: current.history.map(update) }));
  }, []);

  return {
    screen: state.screen,
    noteId: state.screen === 'note' ? state.id : undefined,
    meetingId: state.screen === 'meeting' ? state.id : undefined,
    entry: state.entry,
    direction: state.direction,
    canGoBack: state.history.length > 0,
    navigate,
    back,
    setNoteId,
  };
}
