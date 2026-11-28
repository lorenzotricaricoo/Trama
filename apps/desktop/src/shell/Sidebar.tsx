import { motion } from 'motion/react';
import type { KeyboardEvent } from 'react';
import { Button, Link } from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import { spring } from '../motion';
import { Hint } from '../ui/Hint';
import { MagnifyingGlassIcon, SidebarSimpleIcon } from '../ui/icons';
import { NAV_ITEMS, type NavItem, type ScreenId } from './navigation';
import { SHORTCUTS } from './shortcuts';

interface SidebarProps {
  active: ScreenId;
  collapsed: boolean;
  onNavigate: (screen: ScreenId) => void;
  onToggle: () => void;
  onRecord: () => void;
  onCommandBar: () => void;
}

/** Moves focus between the sidebar items with the arrow keys, Home and End. */
function onNavKeyDown(event: KeyboardEvent<HTMLElement>) {
  const keys = ['ArrowDown', 'ArrowUp', 'Home', 'End'];
  if (!keys.includes(event.key)) return;
  const items = Array.from(event.currentTarget.querySelectorAll<HTMLElement>('[data-nav-item]'));
  const index = items.findIndex((item) => item === document.activeElement);
  if (index < 0) return;
  event.preventDefault();
  const last = items.length - 1;
  const next = {
    ArrowDown: index === last ? 0 : index + 1,
    ArrowUp: index === 0 ? last : index - 1,
    Home: 0,
    End: last,
  }[event.key];
  items[next ?? index]?.focus();
}

export function Sidebar({
  active,
  collapsed,
  onNavigate,
  onToggle,
  onRecord,
  onCommandBar,
}: SidebarProps) {
  const { t } = useI18n();
  const toggleLabel = collapsed ? t('sidebar-expand') : t('sidebar-collapse');

  const item = (nav: NavItem, shortcut?: string) => {
    const label = t(nav.label);
    const current = nav.id === active;
    const Icon = nav.icon;
    return (
      <Hint key={nav.id} label={label} shortcut={shortcut} isDisabled={!collapsed}>
        <Link
          className="side-item"
          data-nav-item
          aria-current={current ? 'page' : undefined}
          onPress={() => onNavigate(nav.id)}
        >
          {current && (
            // One indicator that slides from item to item.
            <motion.span className="side-ind" layoutId="side-ind" transition={spring.soft} />
          )}
          <Icon className="side-ic" aria-hidden />
          <span className="side-label">{label}</span>
          {shortcut && (
            <kbd className="kbd side-kbd" aria-hidden>
              {shortcut}
            </kbd>
          )}
        </Link>
      </Hint>
    );
  };

  return (
    <aside className="side" data-collapsed={collapsed || undefined} aria-label={t('sidebar-label')}>
      <div className="side-head">
        <Hint
          label={toggleLabel}
          shortcut={SHORTCUTS.toggleSidebar.label}
          placement={collapsed ? 'end' : 'bottom'}
        >
          <Button className="side-toggle" aria-label={toggleLabel} onPress={onToggle}>
            <SidebarSimpleIcon aria-hidden />
          </Button>
        </Hint>
      </div>
      <div className="side-body">
        <Hint
          label={t('command-open')}
          shortcut={SHORTCUTS.commandBar.label}
          isDisabled={!collapsed}
        >
          <Button className="side-search" onPress={onCommandBar}>
            <MagnifyingGlassIcon className="side-ic" aria-hidden />
            <span className="side-label">{t('command-open')}</span>
            <kbd className="kbd side-kbd" aria-hidden>
              {SHORTCUTS.commandBar.label}
            </kbd>
          </Button>
        </Hint>
        <Hint label={t('record-meeting')} shortcut={SHORTCUTS.record.label} isDisabled={!collapsed}>
          <Button className="side-rec" onPress={onRecord}>
            <span className="side-recdot" aria-hidden />
            <span className="side-label">{t('record-meeting')}</span>
            <kbd className="kbd side-kbd" aria-hidden>
              {SHORTCUTS.record.label}
            </kbd>
          </Button>
        </Hint>
        <nav className="side-nav" aria-label={t('nav-label')} onKeyDown={onNavKeyDown}>
          {NAV_ITEMS.filter((nav) => nav.place === 'main').map((nav) => item(nav))}
          <div className="side-grow" />
          {NAV_ITEMS.filter((nav) => nav.place === 'bottom').map((nav) =>
            item(nav, nav.id === 'settings' ? SHORTCUTS.settings.label : undefined),
          )}
        </nav>
      </div>
    </aside>
  );
}
