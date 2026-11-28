import { useCallback, useState } from 'react';
import { GearIcon, HouseIcon, StackIcon, type Icon } from '../ui/icons';

/**
 * Screens reachable from the sidebar (see docs/design/information-architecture.md).
 * Only phase 1 screens exist: Projects, People and Doubts join in phase 2, Chat in phase 3,
 * each added here when its phase ships.
 */
export type ScreenId = 'home' | 'sources' | 'settings';

export interface NavItem {
  id: ScreenId;
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

/** Position of a screen in the sidebar, used for the direction of the screen transition. */
function order(screen: ScreenId): number {
  return NAV_ITEMS.findIndex((item) => item.id === screen);
}

/** Direction of a transition: down when moving further down the sidebar, up otherwise. */
export type Direction = 'down' | 'up';

export interface Navigation {
  screen: ScreenId;
  direction: Direction;
  canGoBack: boolean;
  navigate: (screen: ScreenId) => void;
  back: () => void;
}

const MAX_HISTORY = 40;

interface NavState {
  screen: ScreenId;
  history: ScreenId[];
  direction: Direction;
}

function towards(from: ScreenId, to: ScreenId): Direction {
  return order(to) >= order(from) ? 'down' : 'up';
}

/** The current screen and the history behind the Back button. */
export function useNavigation(initial: ScreenId = 'home'): Navigation {
  const [state, setState] = useState<NavState>({
    screen: initial,
    history: [],
    direction: 'down',
  });

  const navigate = useCallback((screen: ScreenId) => {
    setState((current) =>
      current.screen === screen
        ? current
        : {
            screen,
            history: [...current.history, current.screen].slice(-MAX_HISTORY),
            direction: towards(current.screen, screen),
          },
    );
  }, []);

  const back = useCallback(() => {
    setState((current) => {
      const previous = current.history.at(-1);
      return previous === undefined
        ? current
        : {
            screen: previous,
            history: current.history.slice(0, -1),
            direction: towards(current.screen, previous),
          };
    });
  }, []);

  return {
    screen: state.screen,
    direction: state.direction,
    canGoBack: state.history.length > 0,
    navigate,
    back,
  };
}
