import { useCallback, useState } from 'react';

const KEY = 'trama.sidebar.collapsed';

function load(): boolean {
  try {
    return localStorage.getItem(KEY) === 'true';
  } catch {
    return false;
  }
}

function save(collapsed: boolean) {
  try {
    localStorage.setItem(KEY, String(collapsed));
  } catch {
    // Storage unavailable: the sidebar simply opens expanded next time.
  }
}

/** Whether the sidebar is collapsed to the icon rail; remembered across launches. */
export function useSidebarCollapsed(): [boolean, () => void] {
  const [collapsed, setCollapsed] = useState(load);
  const toggle = useCallback(() => {
    setCollapsed((current) => {
      save(!current);
      return !current;
    });
  }, []);
  return [collapsed, toggle];
}
